use compiler::Compiler;
use virtual_machine::execute;
use scanner::Scanner;
use ariadne::Source;
use std::env;
use std::fs;
use std::io::{self, Write};

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
    let compiler = Compiler::new(&mut scanner, source_id);
    match compiler.compile() {
        Ok(chunk) => {
            println!("✅ Compilation successful!");
            println!("📊 Generated {} bytes of bytecode with {} constants",
                     chunk.code.len(), chunk.constants.len());

            // Show debug compilation visualization
            let debug_report = chunk.debug_compilation();
            debug_report.print((source_id, &source))?;

            // Execute the compiled chunk
            match execute(chunk) {
                Ok(result) => {
                    println!("🎯 Execution result: {}", result);
                }
                Err(runtime_error) => {
                    println!("❌ Runtime error during execution:");
                    let report = runtime_error.into_report();
                    report.print((source_id, &source))?;
                }
            }
        }
        Err(compile_error) => {
            println!("❌ Compilation failed:");
            let report = compile_error.into_report();
            report.print((source_id, &source))?;
        }
    }

    Ok(())
}

fn repl_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Jsonnet Compiler & VM REPL - Enter expressions to compile and execute (Ctrl+C to exit)");
    println!("Examples: 42, 3 + 4, -5 * (10 + 2), (1 + 2) * 3");

    loop {
        print!("jsonnet> ");
        io::stdout().flush()?;

        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let input = input.trim();
                if input.is_empty() {
                    continue;
                }

                compile_and_execute_repl(input);
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn compile_and_execute_repl(input: &str) {
    if let Err(e) = compile_and_execute(input, "repl") {
        eprintln!("Failed to process input: {}", e);
    }
}
