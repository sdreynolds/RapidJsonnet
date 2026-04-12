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

use ariadne::{Source, sources};
use compiler::Compiler;
use memory_manager::MemoryManager;
use scanner::Scanner;
use std::env;
use std::fs;
use std::io::{self, Write};
use virtual_machine::{execute, execute_with_ext_vars};

#[derive(Debug)]
enum MainError {
    CompilerError,
    RuntimeError,
}

impl std::fmt::Display for MainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MainError::CompilerError => write!(f, "Compilation failed"),
            MainError::RuntimeError => write!(f, "Runtime error"),
        }
    }
}

impl std::error::Error for MainError {}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse flags before the filename argument
    let mut ext_strs: Vec<(String, String)> = Vec::new();
    let mut ext_codes: Vec<(String, String)> = Vec::new();
    let mut jpaths: Vec<String> = Vec::new();
    let mut quiet = false;
    let mut args_iter = env::args().skip(1).peekable();
    while let Some(arg) = args_iter.peek().cloned() {
        if arg == "-q" || arg == "--quiet" {
            quiet = true;
            args_iter.next();
        } else if arg == "-J" || arg == "--jpath" {
            args_iter.next();
            if let Some(path) = args_iter.next() {
                jpaths.push(path);
            }
        } else if arg.starts_with("-J") && arg.len() > 2 {
            // Support -J<path> (no space)
            jpaths.push(arg[2..].to_string());
            args_iter.next();
        } else if arg.starts_with("--jpath=") {
            jpaths.push(arg.trim_start_matches("--jpath=").to_string());
            args_iter.next();
        } else if arg.starts_with("--ext-str=") {
            let kv = arg.trim_start_matches("--ext-str=");
            if let Some((k, v)) = kv.split_once('=') {
                ext_strs.push((k.to_string(), v.to_string()));
            }
            args_iter.next();
        } else if arg == "--ext-str" {
            args_iter.next();
            if let Some(kv) = args_iter.next() {
                if let Some((k, v)) = kv.split_once('=') {
                    ext_strs.push((k.to_string(), v.to_string()));
                }
            }
        } else if arg.starts_with("--ext-code=") {
            let kv = arg.trim_start_matches("--ext-code=");
            if let Some((k, v)) = kv.split_once('=') {
                ext_codes.push((k.to_string(), v.to_string()));
            }
            args_iter.next();
        } else if arg == "--ext-code" {
            args_iter.next();
            if let Some(kv) = args_iter.next() {
                if let Some((k, v)) = kv.split_once('=') {
                    ext_codes.push((k.to_string(), v.to_string()));
                }
            }
        } else {
            break;
        }
    }
    let file_arg = args_iter.next();

    if let Some(filename) = file_arg {
        // File mode: read from file, compile, and execute
        let content = fs::read_to_string(&filename)?;
        let path = std::path::Path::new(&filename);

        let source_name = if jpaths.is_empty() {
            // No JPATHs: use basename as source_id, chdir to file's directory
            // so that relative imports resolve correctly.
            let basename = path
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&filename)
                .to_string();
            if let Some(parent) = path.parent() {
                if parent != std::path::Path::new("") {
                    std::env::set_current_dir(parent)?;
                }
            }
            basename
        } else {
            // JPATHs provided: use the full path as source_id so that
            // relative-to-file resolution still works, and JPATH search
            // handles workspace-relative imports.
            filename.clone()
        };

        if quiet {
            compile_and_execute_quiet(&content, &source_name, &ext_strs, &ext_codes, &jpaths)?;
        } else {
            compile_and_execute(&content, &source_name, &ext_strs, &ext_codes, &jpaths)?;
        }
    } else {
        // REPL mode: read from stdin, compile, and execute
        repl_mode()?;
    }

    Ok(())
}

/// Format an f64 to match C++ jsonnet's number output (%.17g equivalent).
/// Integers print without a decimal point; others use 17 significant digits.
fn format_double(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        // %.17g: 17 significant digits, "general" format (fixed or scientific).
        // {:.16e} gives 16 decimal digits = 17 significant digits in scientific notation.
        let s = format!("{:.16e}", f);
        let (mantissa, exp_str) = s.split_once('e').unwrap();
        let exp: i32 = exp_str.parse().unwrap();

        // Trim trailing zeros from mantissa (like %g does)
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');

        // %g uses fixed notation when -4 <= exp < precision (17), else scientific
        if exp >= 0 && exp < 17 {
            let digits: String = mantissa.replace('.', "");
            let num_digits = digits.len();
            let decimal_pos = 1 + exp as usize;

            if decimal_pos >= num_digits {
                format!("{}{}", digits, "0".repeat(decimal_pos - num_digits))
            } else {
                let (int_part, frac_part) = digits.split_at(decimal_pos);
                let frac_trimmed = frac_part.trim_end_matches('0');
                if frac_trimmed.is_empty() {
                    int_part.to_string()
                } else {
                    format!("{}.{}", int_part, frac_trimmed)
                }
            }
        } else if exp < 0 && exp >= -4 {
            let digits: String = mantissa.replace('.', "");
            let leading_zeros = (-exp - 1) as usize;
            let frac = format!("{}{}", "0".repeat(leading_zeros), digits);
            let frac_trimmed = frac.trim_end_matches('0');
            format!("0.{}", frac_trimmed)
        } else {
            // Scientific notation: e+NN or e-NN (no leading + for positive exponent in %g)
            if exp >= 0 {
                format!("{}e+{:02}", mantissa, exp)
            } else {
                format!("{}e-{:02}", mantissa, -exp)
            }
        }
    }
}

/// Format a serde_json::Value as Jsonnet-style JSON (3-space indent, sorted keys).
fn jsonnet_manifest(value: &serde_json::Value) -> String {
    jsonnet_manifest_indent(value, 0)
}

fn jsonnet_manifest_indent(value: &serde_json::Value, depth: usize) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => {
            if let Some(f) = n.as_f64() {
                format_double(f)
            } else {
                n.to_string()
            }
        }
        serde_json::Value::String(s) => serde_json::to_string(s).unwrap(),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                "[ ]".to_string()
            } else {
                let indent = "   ".repeat(depth + 1);
                let close_indent = "   ".repeat(depth);
                let items: Vec<String> = arr
                    .iter()
                    .map(|v| format!("{}{}", indent, jsonnet_manifest_indent(v, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", items.join(",\n"), close_indent)
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                "{ }".to_string()
            } else {
                let indent = "   ".repeat(depth + 1);
                let close_indent = "   ".repeat(depth);
                let items: Vec<String> = obj
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            indent,
                            serde_json::to_string(k).unwrap(),
                            jsonnet_manifest_indent(v, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", items.join(",\n"), close_indent)
            }
        }
    }
}

/// Quiet mode: only output the JSON result (like the official jsonnet binary).
fn compile_and_execute_quiet(
    content: &str,
    source_id: &str,
    ext_strs: &[(String, String)],
    ext_codes: &[(String, String)],
    jpaths: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut scanner = Scanner::new(content, source_id);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, source_id);
    match compiler.compile(&mut memory_manager) {
        Ok(chunk) => {
            match execute_with_ext_vars(chunk, memory_manager, ext_strs, ext_codes, jpaths) {
                Ok(result) => {
                    // Format with 3-space indent to match official jsonnet output
                    println!("{}", jsonnet_manifest(&result));
                    Ok(())
                }
                Err(runtime_error) => {
                    print_error_report(&runtime_error, source_id, content, true)?;
                    Err(Box::new(MainError::RuntimeError))
                }
            }
        }
        Err(compile_error) => {
            print_error_report(&compile_error, source_id, content, true)?;
            Err(Box::new(MainError::CompilerError))
        }
    }
}

fn print_error_report(
    error: &scanner::ScanError,
    primary_source_id: &str,
    primary_content: &str,
    use_stderr: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (report, source_ids) = error.into_report();
    let srcs: Vec<(String, String)> = source_ids
        .iter()
        .map(|sid| {
            let content = if sid == primary_source_id {
                primary_content.to_string()
            } else {
                fs::read_to_string(sid).unwrap_or_else(|_| "<file not found>".to_string())
            };
            (sid.clone(), content)
        })
        .collect();
    if use_stderr {
        report.eprint(sources(srcs))?;
    } else {
        report.print(sources(srcs))?;
    }
    Ok(())
}

fn compile_and_execute(
    content: &str,
    source_id: &str,
    ext_strs: &[(String, String)],
    ext_codes: &[(String, String)],
    jpaths: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let source = Source::from(content);

    // Compile the input
    let mut scanner = Scanner::new(content, source_id);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, source_id);
    match compiler.compile(&mut memory_manager) {
        Ok(chunk) => {
            println!("✅ Compilation successful!");
            println!(
                "📊 Generated {} bytes of bytecode with {} constants",
                chunk.code.len(),
                chunk.constants.len()
            );

            // Show debug compilation visualization
            let debug_report = chunk.debug_compilation();
            debug_report.print((source_id, &source))?;

            // Execute the compiled chunk with ext vars
            match execute_with_ext_vars(chunk, memory_manager, ext_strs, ext_codes, jpaths) {
                Ok(result) => {
                    println!("🎯 Execution result: {}", result);
                    Ok(())
                }
                Err(runtime_error) => {
                    println!("❌ Runtime error during execution:");
                    print_error_report(&runtime_error, source_id, content, false)?;
                    Err(Box::new(MainError::RuntimeError))
                }
            }
        }
        Err(compile_error) => {
            println!("❌ Compilation failed:");
            print_error_report(&compile_error, source_id, content, false)?;
            Err(Box::new(MainError::CompilerError))
        }
    }
}

fn repl_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🚀 Jsonnet Compiler & VM REPL - Enter expressions to compile and execute (Ctrl+C to exit)"
    );
    println!("Examples: 42, 3 + 4, -5 * (10 + 2), (1 + 2) * 3");

    let mut buffer = String::new();

    loop {
        if buffer.is_empty() {
            print!("jsonnet> ");
        } else {
            print!("...> ");
        }
        io::stdout().flush()?;

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if buffer.is_empty() && input.trim().is_empty() {
                    continue;
                }

                buffer.push_str(&input);

                match process_repl_input(&buffer) {
                    ReplResult::Incomplete => {
                        continue;
                    }
                    _ => {
                        buffer.clear();
                    }
                }
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }

    Ok(())
}

enum ReplResult {
    Success,
    Incomplete,
    Error,
}

fn process_repl_input(content: &str) -> ReplResult {
    let source_id = "repl";
    let source = Source::from(content);

    let mut scanner = Scanner::new(content, source_id);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, source_id);

    match compiler.compile(&mut memory_manager) {
        Ok(chunk) => {
            println!("✅ Compilation successful!");
            println!(
                "📊 Generated {} bytes of bytecode with {} constants",
                chunk.code.len(),
                chunk.constants.len()
            );

            let debug_report = chunk.debug_compilation();
            let _ = debug_report.print((source_id, &source));

            match execute(chunk, memory_manager) {
                Ok(result) => {
                    println!("🎯 Execution result: {}", result);
                    ReplResult::Success
                }
                Err(runtime_error) => {
                    println!("❌ Runtime error during execution:");
                    let _ = print_error_report(&runtime_error, source_id, content, false);
                    ReplResult::Error
                }
            }
        }
        Err(compile_error) => {
            if compile_error.is_incomplete_input() {
                return ReplResult::Incomplete;
            }

            println!("❌ Compilation failed:");
            let _ = print_error_report(&compile_error, source_id, content, false);
            ReplResult::Error
        }
    }
}
