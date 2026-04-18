// Copyright 2026 Scott Reynolds
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use compiler::Compiler;
use lcov::generate_lcov;
use memory_manager::MemoryManager;
use scanner::Scanner;
use std::env;
use std::fs;
use std::io;
use std::process;
use test_reporter::TextReporter;
use test_runner;
use virtual_machine::VirtualMachine;

fn main() {
    let mut jpaths: Vec<String> = Vec::new();
    let mut collect_coverage = false;
    let mut lcov_output: Option<String> = None;
    let mut suite_name: Option<String> = None;
    let mut test_filter: Option<String> = None;
    let mut args_iter = env::args().skip(1).peekable();

    while let Some(arg) = args_iter.peek().cloned() {
        if arg == "-J" || arg == "--jpath" {
            args_iter.next();
            if let Some(path) = args_iter.next() {
                jpaths.push(path);
            }
        } else if arg.starts_with("-J") && arg.len() > 2 {
            jpaths.push(arg[2..].to_string());
            args_iter.next();
        } else if arg.starts_with("--jpath=") {
            jpaths.push(arg.trim_start_matches("--jpath=").to_string());
            args_iter.next();
        } else if arg == "--coverage" {
            collect_coverage = true;
            args_iter.next();
        } else if arg == "--lcov-output" {
            args_iter.next();
            if let Some(path) = args_iter.next() {
                lcov_output = Some(path);
            }
        } else if arg.starts_with("--lcov-output=") {
            lcov_output = Some(arg["--lcov-output=".len()..].to_string());
            args_iter.next();
        } else if arg == "--test-name" {
            args_iter.next();
            if let Some(name) = args_iter.next() {
                test_filter = Some(name);
            }
        } else if arg.starts_with("--test-name=") {
            test_filter = Some(arg["--test-name=".len()..].to_string());
            args_iter.next();
        } else if arg == "--suite-name" {
            args_iter.next();
            if let Some(name) = args_iter.next() {
                suite_name = Some(name);
            }
        } else if arg.starts_with("--suite-name=") {
            suite_name = Some(arg["--suite-name=".len()..].to_string());
            args_iter.next();
        } else {
            break;
        }
    }

    // Auto-enable coverage when Bazel coverage mode is active.
    if std::env::var("COVERAGE_OUTPUT_FILE").is_ok() {
        collect_coverage = true;
    }

    let filename = match args_iter.next() {
        Some(f) => f,
        None => {
            eprintln!(
                "Usage: jsonnet_test_runner [--coverage] [--lcov-output <path>] [--suite-name <name>] [--test-name <filter>] [-J <path>]... <test_file.jsonnet>"
            );
            process::exit(2);
        }
    };

    let content = match fs::read_to_string(&filename) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading {}: {}", filename, e);
            process::exit(2);
        }
    };

    let mut scanner = Scanner::new(&content, &filename);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, &filename);

    let chunk = match compiler.compile(&mut memory_manager) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compilation failed: {}", e.message);
            process::exit(2);
        }
    };

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    vm.set_jpaths(jpaths);

    let top_level = match vm.interpret() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error evaluating test file: {}", e.message);
            process::exit(2);
        }
    };

    let stdout = io::stdout();
    let mut reporter = TextReporter::new(stdout.lock());

    match test_runner::run_tests(
        &mut vm,
        &top_level,
        &mut reporter,
        collect_coverage,
        test_filter,
        &filename,
        &content,
    ) {
        Ok((all_passed, mut maybe_coverage)) => {
            // Remove the test entrypoint file itself — coverage should only reflect
            // library code under test, not the test driver.
            if let Some(coverage) = &mut maybe_coverage {
                coverage.remove_source(&filename);
            }
            if let Some(coverage) = &maybe_coverage {
                let needs_lcov = lcov_output.is_some()
                    || std::env::var("TEST_UNDECLARED_OUTPUTS_DIR").is_ok()
                    || std::env::var("COVERAGE_DIR").is_ok()
                    || std::env::var("COVERAGE_OUTPUT_FILE").is_ok();
                if needs_lcov {
                    let tn = suite_name.as_deref().unwrap_or("");
                    let lcov_content = generate_lcov(coverage, tn);
                    // Explicit --lcov-output path
                    if let Some(path) = &lcov_output {
                        if let Err(e) = fs::write(path, &lcov_content) {
                            eprintln!("Warning: failed to write LCOV to {}: {}", path, e);
                        }
                    }
                    // bazel test: write to undeclared outputs so the file appears at
                    // bazel-testlogs/<pkg>/<target>/test.outputs/<test_name>.lcov
                    if let Ok(undeclared_dir) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
                        let lcov_filename = if tn.is_empty() {
                            "coverage.lcov".to_string()
                        } else {
                            format!("{}.lcov", tn)
                        };
                        let out = std::path::Path::new(&undeclared_dir).join(&lcov_filename);
                        if let Err(e) = fs::write(&out, &lcov_content) {
                            eprintln!("Warning: failed to write {}: {}", lcov_filename, e);
                        }
                    }
                    // bazel coverage: write LCOV as coverage.dat for Bazel's merger
                    if let Ok(coverage_dir) = std::env::var("COVERAGE_DIR") {
                        let dat = std::path::Path::new(&coverage_dir).join("coverage.dat");
                        if let Err(e) = fs::write(&dat, &lcov_content) {
                            eprintln!("Warning: failed to write coverage.dat: {}", e);
                        }
                    }
                    // bazel coverage: write LCOV to the file Bazel's lcov_merger expects.
                    if let Ok(cov_file) = std::env::var("COVERAGE_OUTPUT_FILE") {
                        if let Err(e) = fs::write(&cov_file, &lcov_content) {
                            eprintln!("Warning: failed to write coverage to {}: {}", cov_file, e);
                        }
                    }
                }
            }
            process::exit(if all_passed { 0 } else { 1 });
        }
        Err(e) => {
            eprintln!("Test runner error: {}", e);
            process::exit(2);
        }
    }
}
