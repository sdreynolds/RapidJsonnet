use ariadne::Source;
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
    let mut quiet = false;
    let mut args_iter = env::args().skip(1).peekable();
    while let Some(arg) = args_iter.peek().cloned() {
        if arg == "-q" || arg == "--quiet" {
            quiet = true;
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
        } else {
            break;
        }
    }
    let file_arg = args_iter.next();

    if let Some(filename) = file_arg {
        // File mode: read from file, compile, and execute
        let content = fs::read_to_string(&filename)?;
        if quiet {
            compile_and_execute_quiet(&content, &filename, &ext_strs)?;
        } else {
            compile_and_execute(&content, &filename, &ext_strs)?;
        }
    } else {
        // REPL mode: read from stdin, compile, and execute
        repl_mode()?;
    }

    Ok(())
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
            // Match jsonnet number formatting
            if let Some(f) = n.as_f64() {
                if f == f.floor() && f.abs() < 1e15 && !n.to_string().contains('e') {
                    // Integer-like: no decimal point
                    format!("{}", f as i64)
                } else {
                    n.to_string()
                }
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
) -> Result<(), Box<dyn std::error::Error>> {
    let mut scanner = Scanner::new(content, source_id);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, source_id);
    match compiler.compile(&mut memory_manager) {
        Ok(chunk) => match execute_with_ext_vars(chunk, memory_manager, ext_strs) {
            Ok(result) => {
                // Format with 3-space indent to match official jsonnet output
                println!("{}", jsonnet_manifest(&result));
                Ok(())
            }
            Err(runtime_error) => {
                let error_source_id = runtime_error.source_id.clone();
                let error_content = if error_source_id == source_id {
                    content.to_string()
                } else {
                    fs::read_to_string(&error_source_id)
                        .unwrap_or_else(|_| "<file not found>".to_string())
                };
                let error_source = Source::from(error_content);
                let report = runtime_error.into_report();
                report.eprint((error_source_id.as_str(), error_source))?;
                Err(Box::new(MainError::RuntimeError))
            }
        },
        Err(compile_error) => {
            let error_source_id = compile_error.source_id.clone();
            let error_content = if error_source_id == source_id {
                content.to_string()
            } else {
                fs::read_to_string(&error_source_id)
                    .unwrap_or_else(|_| "<file not found>".to_string())
            };
            let error_source = Source::from(error_content);
            let report = compile_error.into_report();
            report.eprint((error_source_id.as_str(), error_source))?;
            Err(Box::new(MainError::CompilerError))
        }
    }
}

fn compile_and_execute(
    content: &str,
    source_id: &str,
    ext_strs: &[(String, String)],
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
            match execute_with_ext_vars(chunk, memory_manager, ext_strs) {
                Ok(result) => {
                    println!("🎯 Execution result: {}", result);
                    Ok(())
                }
                Err(runtime_error) => {
                    println!("❌ Runtime error during execution:");
                    let error_source_id = runtime_error.source_id.clone();
                    let error_content = if error_source_id == source_id {
                        content.to_string()
                    } else {
                        fs::read_to_string(&error_source_id)
                            .unwrap_or_else(|_| "<file not found>".to_string())
                    };
                    let error_source = Source::from(error_content);
                    let report = runtime_error.into_report();
                    report.print((error_source_id.as_str(), error_source))?;
                    Err(Box::new(MainError::RuntimeError))
                }
            }
        }
        Err(compile_error) => {
            println!("❌ Compilation failed:");
            let error_source_id = compile_error.source_id.clone();
            let error_content = if error_source_id == source_id {
                content.to_string()
            } else {
                fs::read_to_string(&error_source_id)
                    .unwrap_or_else(|_| "<file not found>".to_string())
            };
            let error_source = Source::from(error_content);
            let report = compile_error.into_report();
            report.print((error_source_id.as_str(), error_source))?;
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
                    let error_source_id = runtime_error.source_id.clone();
                    let error_content = if error_source_id == source_id {
                        content.to_string()
                    } else {
                        fs::read_to_string(&error_source_id)
                            .unwrap_or_else(|_| "<file not found>".to_string())
                    };
                    let error_source = Source::from(error_content);
                    let report = runtime_error.into_report();
                    let _ = report.print((error_source_id.as_str(), error_source));
                    ReplResult::Error
                }
            }
        }
        Err(compile_error) => {
            if compile_error.is_incomplete_input() {
                return ReplResult::Incomplete;
            }

            println!("❌ Compilation failed:");
            let error_source_id = compile_error.source_id.clone();
            let error_content = if error_source_id == source_id {
                content.to_string()
            } else {
                fs::read_to_string(&error_source_id)
                    .unwrap_or_else(|_| "<file not found>".to_string())
            };
            let error_source = Source::from(error_content);
            let report = compile_error.into_report();
            let _ = report.print((error_source_id.as_str(), error_source));
            ReplResult::Error
        }
    }
}
