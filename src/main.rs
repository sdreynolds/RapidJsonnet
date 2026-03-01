use ariadne::Source;
use compiler::Compiler;
use memory_manager::MemoryManager;
use scanner::Scanner;
use std::env;
use std::fs;
use std::io::{self, Write};
use virtual_machine::execute;

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
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        // File mode: read from file, compile, and execute
        let filename = &args[1];
        let content = fs::read_to_string(filename)?;
        compile_and_execute(&content, filename)?;
    } else {
        // REPL mode: read from stdin, compile, and execute
        repl_mode()?;
    }

    Ok(())
}

fn compile_and_execute(content: &str, source_id: &str) -> Result<(), Box<dyn std::error::Error>> {
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

            // Execute the compiled chunk
            match execute(chunk, memory_manager) {
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
