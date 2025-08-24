use scanner::{Scanner, Token, TokenInfo, ScanError};

// Using ScanError from scanner module instead of duplicate ParseError

pub struct Parser<'a> {
    scanner: &'a mut Scanner<'a>,
    previous_token: Option<TokenInfo>,
    current_token: Option<TokenInfo>,
    had_error: bool,
    panic_mode: bool,
}

impl<'a> Parser<'a> {
    pub fn new(scanner: &'a mut Scanner<'a>) -> Self {
        Self {
            scanner,
            previous_token: None,
            current_token: None,
            had_error: false,
            panic_mode: false,
        }
    }

    pub fn advance(&mut self) -> Result<(), ScanError> {
        self.previous_token = self.current_token.take();

        match self.scanner.scan_next() {
            Ok(token_info) => {
                self.current_token = Some(token_info);
                Ok(())
            }
            Err(scan_error) => {
                self.had_error = true;

                if !self.panic_mode {
                    self.panic_mode = true;

                    // ScanError can be used directly
                    Err(scan_error)
                } else {
                    // @TODO: this will be updated when we can successfully unwind and continue parsing. This happens
                    // when we get to statements
                    Ok(())
                }
            }
        }
    }

    pub fn consume(&mut self, expected: Token, message: &str) -> Result<TokenInfo, ScanError> {
        if let Some(ref current) = self.current_token {
            if current.token == expected {
                let token = self.current_token.take().unwrap();
                self.advance()?;
                Ok(token)
            } else {
                let error = ScanError {
                    span: current.span.clone(),
                    message: format!("{}: expected {:?}, found {:?}", message, expected, current.token),
                    source_id: self.scanner.source_id.to_string(),
                };
                self.had_error = true;
                Err(error)
            }
        } else {
            let error = ScanError {
                span: 0..0,
                message: format!("{}: unexpected end of input", message),
                source_id: self.scanner.source_id.to_string(),
            };
            self.had_error = true;
            Err(error)
        }
    }

    pub fn current_token(&self) -> Option<&TokenInfo> {
        self.current_token.as_ref()
    }

    pub fn previous_token(&self) -> Option<&TokenInfo> {
        self.previous_token.as_ref()
    }

    pub fn had_error(&self) -> bool {
        self.had_error
    }

    pub fn panic_mode(&self) -> bool {
        self.panic_mode
    }

    pub fn reset_panic(&mut self) {
        self.panic_mode = false;
    }

    pub fn is_at_end(&self) -> bool {
        matches!(self.current_token, Some(ref token) if matches!(token.token, Token::Eof))
    }

    pub fn source_id(&self) -> &str {
        self.scanner.source_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_advance() {
        let mut scanner = Scanner::new("true false", "test");
        let mut parser = Parser::new(&mut scanner);

        // Initially no tokens
        assert!(parser.current_token().is_none());
        assert!(parser.previous_token().is_none());

        // Advance to first token
        parser.advance().unwrap();
        assert!(matches!(parser.current_token().unwrap().token, Token::True));
        assert!(parser.previous_token().is_none());

        // Advance to second token
        parser.advance().unwrap();
        assert!(matches!(parser.current_token().unwrap().token, Token::False));
        assert!(matches!(parser.previous_token().unwrap().token, Token::True));

        // Advance to EOF
        parser.advance().unwrap();
        assert!(parser.is_at_end());
        assert!(matches!(parser.previous_token().unwrap().token, Token::False));
    }

    #[test]
    fn test_parser_consume_success() {
        let mut scanner = Scanner::new("true", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap();
        let token = parser.consume(Token::True, "Expected true").unwrap();
        assert!(matches!(token.token, Token::True));
    }

    #[test]
    fn test_parser_consume_failure() {
        let mut scanner = Scanner::new("false", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap();
        let result = parser.consume(Token::True, "Expected true");
        assert!(result.is_err());
        assert!(parser.had_error());

        let error = result.unwrap_err();
        assert!(error.message.contains("Expected true"));
        assert!(error.message.contains("expected True"));
        assert!(error.message.contains("found False"));
    }

    #[test]
    fn test_parser_scan_error_handling() {
        let mut scanner = Scanner::new("\"unterminated", "test");
        let mut parser = Parser::new(&mut scanner);

        let result = parser.advance();
        assert!(result.is_err());
        assert!(parser.had_error());
        assert!(parser.panic_mode());

        let error = result.unwrap_err();
        assert!(error.message.contains("Unterminated"));
    }

    #[test]
    fn test_parser_walk_token_stream() {
        let mut scanner = Scanner::new("local x = 42; x + 1", "test");
        let mut parser = Parser::new(&mut scanner);

        // Walk through the entire token stream
        let mut token_count = 0;
        while !parser.is_at_end() && !parser.had_error() {
            parser.advance().unwrap();
            if !parser.is_at_end() {
                token_count += 1;
            }
        }

        // Should have successfully parsed: local, x, =, 42, ;, x, +, 1 (8 tokens)
        assert_eq!(token_count, 8);
        assert!(!parser.had_error());
    }

    #[test]
    fn test_parser_multiple_tokens() {
        let mut scanner = Scanner::new("{\"key\": [1, 2, 3]}", "test");
        let mut parser = Parser::new(&mut scanner);

        // Test consuming specific token sequence
        parser.advance().unwrap();
        parser.consume(Token::LeftBrace, "Expected {").unwrap();

        parser.consume(Token::String("key".to_string()), "Expected string").unwrap();

        parser.consume(Token::Operator(":".to_string()), "Expected :").unwrap();

        parser.consume(Token::LeftBracket, "Expected [").unwrap();

        // Skip through the array elements
        parser.consume(Token::Number(1.0), "Expected 1").unwrap();
        parser.consume(Token::Comma, "Expected ,").unwrap();
        parser.consume(Token::Number(2.0), "Expected 2").unwrap();
        parser.consume(Token::Comma, "Expected ,").unwrap();
        parser.consume(Token::Number(3.0), "Expected 3").unwrap();

        parser.consume(Token::RightBracket, "Expected ]").unwrap();
        parser.consume(Token::RightBrace, "Expected }").unwrap();

        assert!(!parser.had_error());
    }
}
