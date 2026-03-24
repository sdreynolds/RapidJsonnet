use compiler::Compiler;
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
        } else {
            break;
        }
    }

    let filename = match args_iter.next() {
        Some(f) => f,
        None => {
            eprintln!("Usage: jsonnet_test_runner [--coverage] [-J <path>]... <test_file.jsonnet>");
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

    match test_runner::run_tests(&mut vm, &top_level, &mut reporter, collect_coverage) {
        Ok(true) => process::exit(0),
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("Test runner error: {}", e);
            process::exit(2);
        }
    }
}
