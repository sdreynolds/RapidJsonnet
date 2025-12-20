use scanner::{ScanError, Scanner, Token, TokenInfo};

// Using ScanError from scanner module instead of duplicate ParseError

pub struct Parser<'a> {
    scanner: &'a mut Scanner<'a>,
    previous_token: Option<TokenInfo>,
    current_token: Option<TokenInfo>,
    token_buffer: Vec<TokenInfo>, // Buffer of upcoming tokens for lookahead
    buffer_position: usize,       // Current position in buffer (0 = current_token)
    had_error: bool,
    panic_mode: bool,
}

impl<'a> Parser<'a> {
    pub fn new(scanner: &'a mut Scanner<'a>) -> Self {
        Self {
            scanner,
            previous_token: None,
            current_token: None,
            token_buffer: Vec::new(),
            buffer_position: 0,
            had_error: false,
            panic_mode: false,
        }
    }

    pub fn advance(&mut self) -> Result<(), ScanError> {
        self.previous_token = self.current_token.take();

        // If we have buffered tokens, use them first (for backtracking support)
        if self.buffer_position < self.token_buffer.len() {
            self.current_token = Some(self.token_buffer[self.buffer_position].clone());
            self.buffer_position += 1;
            return Ok(());
        }

        // Otherwise, scan from the scanner
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
                    message: format!(
                        "{}: expected {:?}, found {:?}",
                        message, expected, current.token
                    ),
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

    /// Peek ahead n tokens from the current position
    /// - n=0 returns current_token
    /// - n=1 returns the next token after current
    /// - n=2 returns the token after that, etc.
    /// Returns None if we hit EOF before reaching n
    pub fn peek_ahead(&mut self, n: usize) -> Result<Option<&TokenInfo>, ScanError> {
        // Special case: n=0 returns current token
        if n == 0 {
            return Ok(self.current_token.as_ref());
        }

        // For n > 0, we need to look in the buffer
        // Buffer index is n-1 (since buffer starts at current+1)
        let buffer_index = n - 1;

        // Check if we already have EOF in the buffer - can't look past it
        if let Some(last) = self.token_buffer.last() {
            if matches!(last.token, Token::Eof) && buffer_index >= self.token_buffer.len() {
                return Ok(None);
            }
        }

        // Ensure we have enough tokens in the buffer
        while self.token_buffer.len() <= buffer_index {
            match self.scanner.scan_next() {
                Ok(token_info) => {
                    let is_eof = matches!(token_info.token, Token::Eof);
                    self.token_buffer.push(token_info);
                    if is_eof {
                        break;
                    }
                }
                Err(scan_error) => {
                    self.had_error = true;
                    if !self.panic_mode {
                        self.panic_mode = true;
                        return Err(scan_error);
                    }
                    return Ok(None);
                }
            }
        }

        Ok(self.token_buffer.get(buffer_index))
    }

    /// Save current parser position for potential backtracking
    /// Returns a checkpoint that can be passed to restore_checkpoint
    pub fn save_checkpoint(&self) -> ParserCheckpoint {
        ParserCheckpoint {
            previous_token: self.previous_token.clone(),
            current_token: self.current_token.clone(),
            buffer_position: self.buffer_position,
            had_error: self.had_error,
            panic_mode: self.panic_mode,
        }
    }

    /// Restore parser to a previously saved checkpoint
    /// This allows speculative parsing with rollback capability
    pub fn restore_checkpoint(&mut self, checkpoint: ParserCheckpoint) {
        self.previous_token = checkpoint.previous_token;
        self.current_token = checkpoint.current_token;
        self.buffer_position = checkpoint.buffer_position;
        // Keep the buffer intact - it contains lookahead tokens we've already scanned
        // The buffer_position tells us where to continue reading from
        self.had_error = checkpoint.had_error;
        self.panic_mode = checkpoint.panic_mode;
    }

    /// Clear the token buffer to free memory
    /// Useful after determining we don't need to backtrack
    pub fn commit(&mut self) {
        self.token_buffer.clear();
        self.buffer_position = 0;
    }
}

/// Checkpoint for parser state, used for backtracking
#[derive(Clone)]
pub struct ParserCheckpoint {
    previous_token: Option<TokenInfo>,
    current_token: Option<TokenInfo>,
    buffer_position: usize,
    had_error: bool,
    panic_mode: bool,
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
        assert!(matches!(
            parser.current_token().unwrap().token,
            Token::False
        ));
        assert!(matches!(
            parser.previous_token().unwrap().token,
            Token::True
        ));

        // Advance to EOF
        parser.advance().unwrap();
        assert!(parser.is_at_end());
        assert!(matches!(
            parser.previous_token().unwrap().token,
            Token::False
        ));
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

        parser
            .consume(Token::String("key".to_string()), "Expected string")
            .unwrap();

        parser
            .consume(Token::Operator(":".to_string()), "Expected :")
            .unwrap();

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

    #[test]
    fn test_peek_ahead_basic() {
        let mut scanner = Scanner::new("true false null", "test");
        let mut parser = Parser::new(&mut scanner);

        // Advance to first token
        parser.advance().unwrap();

        // Peek ahead at position 0 (current token)
        let token0 = parser.peek_ahead(0).unwrap();
        assert!(token0.is_some());
        assert!(matches!(token0.unwrap().token, Token::True));

        // Peek ahead at position 1 (next token)
        let token1 = parser.peek_ahead(1).unwrap();
        assert!(token1.is_some());
        assert!(matches!(token1.unwrap().token, Token::False));

        // Peek ahead at position 2 (token after next)
        let token2 = parser.peek_ahead(2).unwrap();
        assert!(token2.is_some());
        assert!(matches!(token2.unwrap().token, Token::Null));

        // Current token should still be 'true'
        assert!(matches!(parser.current_token().unwrap().token, Token::True));
    }

    #[test]
    fn test_peek_ahead_past_eof() {
        let mut scanner = Scanner::new("true", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap();

        // Peek ahead past EOF should return None
        let token1 = parser.peek_ahead(1).unwrap();
        assert!(token1.is_some());
        assert!(matches!(token1.unwrap().token, Token::Eof));

        // Further ahead should return None
        let token2 = parser.peek_ahead(2).unwrap();
        assert!(token2.is_none());
    }

    #[test]
    fn test_peek_ahead_array_comprehension_pattern() {
        // Test the pattern needed for array comprehensions: [expr for x in array]
        let mut scanner = Scanner::new("[x for x in [1, 2, 3]]", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap(); // [
        parser.advance().unwrap(); // x

        // At this point current_token is 'x' (the first identifier)
        // We need to look ahead to see if there's a 'for' keyword
        let token0 = parser.peek_ahead(0).unwrap();
        assert!(matches!(token0.unwrap().token, Token::Identifier(ref s) if s == "x"));

        let token1 = parser.peek_ahead(1).unwrap();
        assert!(matches!(token1.unwrap().token, Token::For));

        // Current token should still be first 'x'
        assert!(
            matches!(parser.current_token().unwrap().token, Token::Identifier(ref s) if s == "x")
        );
    }

    #[test]
    fn test_checkpoint_and_restore() {
        let mut scanner = Scanner::new("true false null", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap(); // true

        // Save checkpoint
        let checkpoint = parser.save_checkpoint();

        // Advance and modify state
        parser.advance().unwrap(); // false
        parser.advance().unwrap(); // null
        assert!(matches!(parser.current_token().unwrap().token, Token::Null));

        // Restore to checkpoint
        parser.restore_checkpoint(checkpoint);

        // Should be back at 'true'
        assert!(matches!(parser.current_token().unwrap().token, Token::True));
        assert!(parser.previous_token().is_none());
    }

    #[test]
    fn test_checkpoint_preserves_error_state() {
        let mut scanner = Scanner::new("true false", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap();

        // Save checkpoint in non-error state
        let checkpoint = parser.save_checkpoint();
        assert!(!parser.had_error());
        assert!(!parser.panic_mode());

        // Set error state
        parser.had_error = true;
        parser.panic_mode = true;
        assert!(parser.had_error());
        assert!(parser.panic_mode());

        // Restore checkpoint
        parser.restore_checkpoint(checkpoint);

        // Error state should be restored
        assert!(!parser.had_error());
        assert!(!parser.panic_mode());
    }

    #[test]
    fn test_speculative_parsing_with_backtrack() {
        // Simulate trying to parse as array comprehension, failing, then backtracking
        let mut scanner = Scanner::new("[1, 2, 3]", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap(); // [
        parser.advance().unwrap(); // 1

        // Save checkpoint before speculative parse
        let checkpoint = parser.save_checkpoint();

        // Try to look ahead for 'for' keyword (not present)
        let next_token = parser.peek_ahead(1).unwrap();
        assert!(matches!(next_token.unwrap().token, Token::Comma));

        // Not a comprehension, restore and continue as regular array
        parser.restore_checkpoint(checkpoint);

        // Should be back at '1'
        assert!(matches!(
            parser.current_token().unwrap().token,
            Token::Number(1.0)
        ));

        // Can continue parsing normally
        parser.advance().unwrap(); // ,
        assert!(matches!(
            parser.current_token().unwrap().token,
            Token::Comma
        ));
    }

    #[test]
    fn test_commit_clears_buffer() {
        let mut scanner = Scanner::new("true false null", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap();

        // Peek ahead to fill buffer
        parser.peek_ahead(2).unwrap();
        assert!(!parser.token_buffer.is_empty());

        // Commit to clear buffer
        parser.commit();
        assert!(parser.token_buffer.is_empty());
        assert_eq!(parser.buffer_position, 0);
    }

    #[test]
    fn test_peek_ahead_with_operators() {
        let mut scanner = Scanner::new("x + 1 == 2", "test");
        let mut parser = Parser::new(&mut scanner);

        parser.advance().unwrap(); // x

        // Look ahead for operator sequence
        let token1 = parser.peek_ahead(1).unwrap();
        assert!(matches!(
            token1.unwrap().token,
            Token::Operator(ref op) if op == "+"
        ));

        let token2 = parser.peek_ahead(2).unwrap();
        assert!(matches!(token2.unwrap().token, Token::Number(1.0)));

        let token3 = parser.peek_ahead(3).unwrap();
        assert!(matches!(
            token3.unwrap().token,
            Token::Operator(ref op) if op == "=="
        ));
    }
}
