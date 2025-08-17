mod scanner;

use scanner::Scanner;
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

    let source = Source::from(content);
    
    match scanner.scan_all() {
        Ok(tokens) => {
            let success_report = ariadne::Report::build(ariadne::ReportKind::Advice, (source_id, 0..0))
                .with_message(format!("Successfully tokenized {} tokens", tokens.len() - 1)) // -1 for EOF
                .with_note("All tokens parsed successfully");
            
            // Add labels for each token with different colors
            let mut colored_report = success_report;
            let mut color_gen = ariadne::ColorGenerator::new();
            
            for token_info in &tokens {
                if matches!(token_info.token, scanner::Token::Eof) {
                    continue;
                }
                
                let color = color_gen.next();
                let token_type = get_token_type_name(&token_info.token);
                
                colored_report = colored_report.with_label(
                    ariadne::Label::new((source_id, token_info.span.clone()))
                        .with_message(format!("{}: {:?}", token_type, token_info.token))
                        .with_color(color)
                );
            }
            
            colored_report.finish().print((source_id, &source))?;
        }
        Err(errors) => {
            for error in errors {
                let report = error.into_report();
                report.print((error.source_id.as_str(), &source))?;
            }
            return Err("Tokenization failed".into());
        }
    }

    Ok(())
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

fn get_token_type_name(token: &scanner::Token) -> &'static str {
    match token {
        scanner::Token::Identifier(_) => "Identifier",
        scanner::Token::Number(_) => "Number", 
        scanner::Token::String(_) => "String",
        scanner::Token::Assert => "Keyword",
        scanner::Token::Else => "Keyword",
        scanner::Token::Error => "Keyword",
        scanner::Token::False => "Keyword",
        scanner::Token::For => "Keyword",
        scanner::Token::Function => "Keyword",
        scanner::Token::If => "Keyword",
        scanner::Token::Import => "Keyword",
        scanner::Token::ImportStr => "Keyword",
        scanner::Token::ImportBin => "Keyword",
        scanner::Token::In => "Keyword",
        scanner::Token::Local => "Keyword",
        scanner::Token::Null => "Keyword",
        scanner::Token::TailStrict => "Keyword",
        scanner::Token::Then => "Keyword",
        scanner::Token::Self_ => "Keyword",
        scanner::Token::Super => "Keyword",
        scanner::Token::True => "Keyword",
        scanner::Token::LeftBrace | scanner::Token::RightBrace |
        scanner::Token::LeftBracket | scanner::Token::RightBracket |
        scanner::Token::Comma | scanner::Token::Dot |
        scanner::Token::LeftParen | scanner::Token::RightParen |
        scanner::Token::Semicolon => "Symbol",
        scanner::Token::Operator(_) => "Operator",
        scanner::Token::Eof => "EOF",
    }
}

fn tokenize_and_print_repl(input: &str) {
    if let Err(e) = tokenize_and_print(input, "repl") {
        eprintln!("Failed to print tokens: {}", e);
    }
}
