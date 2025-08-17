mod scanner;

use scanner::{Scanner, Token, TokenInfo};
use ariadne::Source;
use std::env;
use std::fs;
use std::io::{self, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 {
        // File mode: read from file and tokenize
        let filename = &args[1];
        let content = fs::read_to_string(filename)?;
        tokenize_and_print(&content, filename)?;
    } else {
        // REPL mode: read from stdin
        repl_mode()?;
    }

    Ok(())
}

fn tokenize_and_print(content: &str, source_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut scanner = Scanner::new(content, source_id);

    match scanner.scan_all() {
        Ok(tokens) => {
            for token_info in tokens {
                print_token(&token_info);
            }
        }
        Err(errors) => {
            let source = Source::from(content);
            for error in errors {
                let report = error.into_report();
                report.print((error.source_id.as_str(), &source))?;
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

fn print_token(token_info: &TokenInfo) {
    match &token_info.token {
        Token::Eof => return, // Don't print EOF token
        token => {
            println!("{:?} @ {}..{}", token, token_info.span.start, token_info.span.end);
        }
    }
}

fn repl_mode() -> Result<(), Box<dyn std::error::Error>> {
    println!("Jsonnet Scanner REPL - Enter Jsonnet code to tokenize (Ctrl+C to exit)");

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

                tokenize_and_print_repl(input);
            }
            Err(e) => {
                eprintln!("Error reading input: {}", e);
                break;
            }
        }
    }

    Ok(())
}

fn tokenize_and_print_repl(input: &str) {
    let mut scanner = Scanner::new(input, "repl");

    match scanner.scan_all() {
        Ok(tokens) => {
            for token_info in tokens {
                print_token(&token_info);
            }
        }
        Err(errors) => {
            let source = Source::from(input);
            for error in errors {
                let report = error.into_report();
                if let Err(e) = report.print((error.source_id.as_str(), &source)) {
                    eprintln!("Failed to print error: {}", e);
                }
            }
        }
    }
}
