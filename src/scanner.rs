use ariadne::{Label, Report, ReportKind};
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // Literals
    Identifier(String),
    Number(f64),
    String(String),

    // Keywords
    Assert,
    Else,
    Error,
    False,
    For,
    Function,
    If,
    Import,
    ImportStr,
    ImportBin,
    In,
    Local,
    Null,
    TailStrict,
    Then,
    Self_,
    Super,
    True,

    // Symbols
    LeftBrace,    // {
    RightBrace,   // }
    LeftBracket,  // [
    RightBracket, // ]
    Comma,        // ,
    Dot,          // .
    LeftParen,    // (
    RightParen,   // )
    Semicolon,    // ;

    // Operators
    Operator(String),

    // Special
    Eof,
}

#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: Token,
    pub span: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct ScanError {
    pub span: Range<usize>,
    pub message: String,
    pub source_id: String,
}

impl ScanError {
    pub fn is_incomplete_input(&self) -> bool {
        let msg = self.message.to_lowercase();
        msg.contains("unexpected end of input")
            || msg.contains("unterminated")
            || msg.contains("starting with eof")
            || msg.contains("found eof")
    }

    pub fn into_report(&self) -> Report<'static, (&str, Range<usize>)> {
        let color = ariadne::Color::Red;

        Report::build(
            ReportKind::Error,
            (self.source_id.as_str(), self.span.clone()),
        )
        .with_message(&self.message)
        .with_label(
            Label::new((self.source_id.as_str(), self.span.clone()))
                .with_message(&self.message)
                .with_color(color),
        )
        .finish()
    }
}

pub struct Scanner<'a> {
    input: &'a str,
    position: usize,
    line: usize,
    column: usize,
    pub source_id: &'a str,
    collected_strings: Vec<String>,
}

impl<'a> Scanner<'a> {
    pub fn new(input: &'a str, source_id: &'a str) -> Self {
        Self {
            input,
            position: 0,
            line: 1,
            column: 1,
            source_id,
            collected_strings: Vec::new(),
        }
    }

    /// Get reference to all collected strings from scanning
    pub fn collected_strings(&self) -> &Vec<String> {
        &self.collected_strings
    }

    pub fn scan_all(&mut self) -> Result<Vec<TokenInfo>, Vec<ScanError>> {
        let mut tokens = Vec::new();
        let mut errors = Vec::new();

        loop {
            match self.scan_next() {
                Ok(token_info) => {
                    let is_eof = matches!(token_info.token, Token::Eof);
                    tokens.push(token_info);
                    if is_eof {
                        break;
                    }
                }
                Err(error) => {
                    errors.push(error);
                    // Try to recover by advancing one character
                    self.advance();
                }
            }
        }

        // Check for trailing semicolon - the last non-EOF token shouldn't be a semicolon
        if tokens.len() >= 2 {
            if let Some(second_last) = tokens.get(tokens.len() - 2) {
                if matches!(second_last.token, Token::Semicolon) {
                    errors.push(self.make_error(
                        second_last.span.clone(),
                        "Trailing semicolon at end of file is not valid in Jsonnet".to_string(),
                    ));
                }
            }
        }

        if errors.is_empty() {
            Ok(tokens)
        } else {
            Err(errors)
        }
    }

    fn make_error(&self, span: Range<usize>, message: String) -> ScanError {
        ScanError {
            span,
            message,
            source_id: self.source_id.to_string(),
        }
    }

    pub fn scan_next(&mut self) -> Result<TokenInfo, ScanError> {
        self.skip_whitespace_and_comments()?;

        let start = self.position;

        if self.is_at_end() {
            return Ok(TokenInfo {
                token: Token::Eof,
                span: start..start,
            });
        }

        let ch = self.peek();

        match ch {
            // Single character symbols
            '{' => {
                self.advance();
                Ok(self.make_token(Token::LeftBrace, start))
            }
            '}' => {
                self.advance();
                Ok(self.make_token(Token::RightBrace, start))
            }
            '[' => {
                self.advance();
                Ok(self.make_token(Token::LeftBracket, start))
            }
            ']' => {
                self.advance();
                Ok(self.make_token(Token::RightBracket, start))
            }
            ',' => {
                self.advance();
                Ok(self.make_token(Token::Comma, start))
            }
            '.' => {
                self.advance();
                Ok(self.make_token(Token::Dot, start))
            }
            '(' => {
                self.advance();
                Ok(self.make_token(Token::LeftParen, start))
            }
            ')' => {
                self.advance();
                Ok(self.make_token(Token::RightParen, start))
            }
            ';' => {
                self.advance();
                Ok(self.make_token(Token::Semicolon, start))
            }

            // String literals
            '"' => self.scan_string('"', false, start),
            '\'' => self.scan_string('\'', false, start),
            '@' if self.peek_ahead(1) == Some('"') => {
                self.advance(); // skip @
                self.scan_string('"', true, start)
            }
            '@' if self.peek_ahead(1) == Some('\'') => {
                self.advance(); // skip @
                self.scan_string('\'', true, start)
            }

            // Text blocks
            '|' if self.peek_ahead(1) == Some('|') && self.peek_ahead(2) == Some('|') => {
                self.scan_text_block(start)
            }

            // Numbers
            ch if ch.is_ascii_digit() => self.scan_number(start),

            // Identifiers and keywords
            ch if ch.is_alphabetic() || ch == '_' => self.scan_identifier_or_keyword(start),

            // Operators
            ch if "!$:~+-&|^=<>*/%".contains(ch) => self.scan_operator(start),

            _ => Err(self.make_error(
                start..start + ch.len_utf8(),
                format!("Unexpected character '{}'", ch),
            )),
        }
    }

    fn make_token(&self, token: Token, start: usize) -> TokenInfo {
        TokenInfo {
            token,
            span: start..self.position,
        }
    }

    fn peek(&self) -> char {
        self.input[self.position..].chars().next().unwrap_or('\0')
    }

    fn peek_ahead(&self, offset: usize) -> Option<char> {
        self.input[self.position..].chars().nth(offset)
    }

    fn advance(&mut self) -> char {
        let ch = self.peek();
        if ch != '\0' {
            self.position += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        ch
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.input.len()
    }

    fn skip_whitespace_and_comments(&mut self) -> Result<(), ScanError> {
        loop {
            let ch = self.peek();
            match ch {
                ' ' | '\t' | '\r' | '\n' => {
                    self.advance();
                }
                '#' => {
                    self.skip_line_comment();
                }
                '/' if self.peek_ahead(1) == Some('/') => {
                    self.skip_line_comment();
                }
                '/' if self.peek_ahead(1) == Some('*') => {
                    self.skip_block_comment()?;
                }
                _ => break,
            }
        }
        Ok(())
    }

    fn skip_line_comment(&mut self) {
        while self.peek() != '\n' && !self.is_at_end() {
            self.advance();
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), ScanError> {
        let start = self.position;
        self.advance(); // /
        self.advance(); // *

        while !self.is_at_end() {
            if self.peek() == '*' && self.peek_ahead(1) == Some('/') {
                self.advance(); // *
                self.advance(); // /
                return Ok(());
            }
            self.advance();
        }

        Err(self.make_error(
            start..self.position,
            "Unterminated block comment".to_string(),
        ))
    }

    fn scan_string(
        &mut self,
        quote: char,
        verbatim: bool,
        start: usize,
    ) -> Result<TokenInfo, ScanError> {
        self.advance(); // opening quote
        let mut value = String::new();

        while !self.is_at_end() {
            if self.peek() == quote {
                if verbatim && self.peek_ahead(1) == Some(quote) {
                    // Doubled quote in verbatim string
                    self.advance(); // first quote
                    self.advance(); // second quote
                    value.push(quote);
                } else {
                    // End of string
                    break;
                }
            } else {
                let ch = self.advance();
                if verbatim {
                    value.push(ch);
                } else {
                    // Regular strings: handle escape sequences
                    if ch == '\\' {
                        let escaped = self.advance();
                        match escaped {
                            '"' | '\'' | '\\' | '/' => value.push(escaped),
                            'b' => value.push('\u{0008}'),
                            'f' => value.push('\u{000C}'),
                            'n' => value.push('\n'),
                            'r' => value.push('\r'),
                            't' => value.push('\t'),
                            'u' => {
                                let hex_start = self.position;
                                let hex_digits: String = (0..4).map(|_| self.advance()).collect();

                                if let Ok(code_point) = u32::from_str_radix(&hex_digits, 16) {
                                    if let Some(unicode_char) = char::from_u32(code_point) {
                                        value.push(unicode_char);
                                    } else {
                                        return Err(self.make_error(
                                            hex_start - 2..self.position,
                                            format!(
                                                "Invalid unicode escape sequence: \\u{}",
                                                hex_digits
                                            ),
                                        ));
                                    }
                                } else {
                                    return Err(self.make_error(
                                        hex_start - 2..self.position,
                                        format!(
                                            "Invalid unicode escape sequence: \\u{}",
                                            hex_digits
                                        ),
                                    ));
                                }
                            }
                            _ => {
                                return Err(self.make_error(
                                    self.position - 2..self.position,
                                    format!("Invalid escape sequence: \\{}", escaped),
                                ));
                            }
                        }
                    } else {
                        value.push(ch);
                    }
                }
            }
        }

        if self.is_at_end() {
            return Err(self.make_error(
                start..self.position,
                format!("Unterminated string literal starting with {}", quote),
            ));
        }

        self.advance(); // closing quote

        self.collected_strings.push(value.clone());

        Ok(TokenInfo {
            token: Token::String(value),
            span: start..self.position,
        })
    }

    fn scan_text_block(&mut self, start: usize) -> Result<TokenInfo, ScanError> {
        // Skip |||
        self.advance();
        self.advance();
        self.advance();

        // Check for optional -
        let strip_final_newline = if self.peek() == '-' {
            self.advance();
            true
        } else {
            false
        };

        // Skip whitespace until newline
        while self.peek() == ' ' || self.peek() == '\t' {
            self.advance();
        }

        if self.peek() != '\n' {
            return Err(self.make_error(
                start..self.position,
                "Text block must have newline after |||".to_string(),
            ));
        }
        self.advance(); // newline

        // Find indentation from first non-empty line
        let indent_start = self.position;
        let mut indent = String::new();
        while self.peek() == ' ' || self.peek() == '\t' {
            indent.push(self.advance());
        }

        if indent.is_empty() {
            return Err(self.make_error(
                indent_start..self.position,
                "Text block requires indentation on first non-empty line".to_string(),
            ));
        }

        let mut lines = Vec::new();
        let _current_line = String::new();

        // Reset to start of first line
        self.position = indent_start;

        loop {
            let line_start = self.position;
            let mut line = String::new();

            // Read entire line
            while !self.is_at_end() && self.peek() != '\n' {
                line.push(self.advance());
            }

            if !self.is_at_end() {
                self.advance(); // consume newline
            }

            // Check if this is the end marker
            let trimmed = line.trim();
            if trimmed == "|||" {
                break;
            }

            // Handle indentation
            if line.trim().is_empty() {
                lines.push(String::new());
            } else if line.starts_with(&indent) {
                lines.push(line[indent.len()..].to_string());
            } else {
                return Err(self.make_error(
                    line_start..self.position,
                    format!(
                        "Text block line doesn't start with expected indentation: '{}'",
                        indent
                    ),
                ));
            }
        }

        let mut result = lines.join("\n");
        if strip_final_newline && result.ends_with('\n') {
            result.pop();
        }

        self.collected_strings.push(result.clone());

        Ok(TokenInfo {
            token: Token::String(result),
            span: start..self.position,
        })
    }

    fn scan_number(&mut self, start: usize) -> Result<TokenInfo, ScanError> {
        let mut _has_dot = false;
        let mut _has_exp = false;

        // Integer part
        while self.peek().is_ascii_digit() {
            self.advance();
        }

        // Fractional part
        if self.peek() == '.' && self.peek_ahead(1).map_or(false, |c| c.is_ascii_digit()) {
            _has_dot = true;
            self.advance(); // .
            while self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        // Exponent part
        if self.peek() == 'e' || self.peek() == 'E' {
            _has_exp = true;
            self.advance(); // e/E

            if self.peek() == '+' || self.peek() == '-' {
                self.advance();
            }

            if !self.peek().is_ascii_digit() {
                return Err(self.make_error(
                    start..self.position,
                    "Invalid number: expected digits after exponent".to_string(),
                ));
            }

            while self.peek().is_ascii_digit() {
                self.advance();
            }
        }

        let number_str = &self.input[start..self.position];
        match number_str.parse::<f64>() {
            Ok(value) => Ok(TokenInfo {
                token: Token::Number(value),
                span: start..self.position,
            }),
            Err(_) => Err(self.make_error(
                start..self.position,
                format!("Invalid number format: {}", number_str),
            )),
        }
    }

    fn scan_identifier_or_keyword(&mut self, start: usize) -> Result<TokenInfo, ScanError> {
        while self.peek().is_alphanumeric() || self.peek() == '_' {
            self.advance();
        }

        let text = &self.input[start..self.position];
        let token = match text {
            "assert" => Token::Assert,
            "else" => Token::Else,
            "error" => Token::Error,
            "false" => Token::False,
            "for" => Token::For,
            "function" => Token::Function,
            "if" => Token::If,
            "import" => Token::Import,
            "importstr" => Token::ImportStr,
            "importbin" => Token::ImportBin,
            "in" => Token::In,
            "local" => Token::Local,
            "null" => Token::Null,
            "tailstrict" => Token::TailStrict,
            "then" => Token::Then,
            "self" => Token::Self_,
            "super" => Token::Super,
            "true" => Token::True,
            _ => Token::Identifier(text.to_string()),
        };

        Ok(TokenInfo {
            token,
            span: start..self.position,
        })
    }

    fn scan_operator(&mut self, start: usize) -> Result<TokenInfo, ScanError> {
        let mut operator = String::new();

        while !self.is_at_end() {
            let ch = self.peek();
            if !"!$:~+-&|^=<>*/%".contains(ch) {
                break;
            }

            let new_operator = format!("{}{}", operator, ch);

            // Check forbidden sequences
            if new_operator.contains("//")
                || new_operator.contains("/*")
                || new_operator.contains("|||")
            {
                break;
            }

            // Check ending restrictions for multi-character operators
            if new_operator.len() > 1 && "+-~!$".contains(ch) {
                break;
            }

            operator.push(ch);
            self.advance();

            // Single character operators that shouldn't be extended
            if operator.len() == 1
                && matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ';')
            {
                break;
            }
        }

        if operator.is_empty() {
            let ch = self.advance();
            return Err(self.make_error(
                start..self.position,
                format!("Unexpected character '{}'", ch),
            ));
        }

        Ok(TokenInfo {
            token: Token::Operator(operator),
            span: start..self.position,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_single_token(input: &str) -> Result<Token, ScanError> {
        let mut scanner = Scanner::new(input, "test");
        let token_info = scanner.scan_next()?;
        Ok(token_info.token)
    }

    #[test]
    fn test_keywords() {
        assert_eq!(scan_single_token("assert").unwrap(), Token::Assert);
        assert_eq!(scan_single_token("false").unwrap(), Token::False);
        assert_eq!(scan_single_token("true").unwrap(), Token::True);
        assert_eq!(scan_single_token("null").unwrap(), Token::Null);
        assert_eq!(scan_single_token("self").unwrap(), Token::Self_);
        assert_eq!(scan_single_token("super").unwrap(), Token::Super);
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(
            scan_single_token("foo").unwrap(),
            Token::Identifier("foo".to_string())
        );
        assert_eq!(
            scan_single_token("_bar").unwrap(),
            Token::Identifier("_bar".to_string())
        );
        assert_eq!(
            scan_single_token("baz123").unwrap(),
            Token::Identifier("baz123".to_string())
        );
    }

    #[test]
    fn test_numbers() {
        assert_eq!(scan_single_token("123").unwrap(), Token::Number(123.0));
        assert_eq!(scan_single_token("123.45").unwrap(), Token::Number(123.45));
        assert_eq!(scan_single_token("1e10").unwrap(), Token::Number(1e10));
        assert_eq!(scan_single_token("1.5e-3").unwrap(), Token::Number(1.5e-3));
    }

    #[test]
    fn test_strings() {
        assert_eq!(
            scan_single_token("\"hello\"").unwrap(),
            Token::String("hello".to_string())
        );
        assert_eq!(
            scan_single_token("'world'").unwrap(),
            Token::String("world".to_string())
        );
        assert_eq!(
            scan_single_token("@\"verbatim\"").unwrap(),
            Token::String("verbatim".to_string())
        );
    }

    #[test]
    fn test_unicode() {
        let mut scanner = Scanner::new("\"🚀\"", "test");
        let tokens = scanner.scan_all().unwrap();
        assert_eq!(tokens.len(), 2); // String and Eof
        if let Token::String(s) = &tokens[0].token {
            assert_eq!(s, "🚀");
        } else {
            panic!("Expected string token");
        }
    }

    #[test]
    fn test_symbols() {
        assert_eq!(scan_single_token("{").unwrap(), Token::LeftBrace);
        assert_eq!(scan_single_token("}").unwrap(), Token::RightBrace);
        assert_eq!(scan_single_token("[").unwrap(), Token::LeftBracket);
        assert_eq!(scan_single_token("]").unwrap(), Token::RightBracket);
        assert_eq!(scan_single_token(",").unwrap(), Token::Comma);
        assert_eq!(scan_single_token(".").unwrap(), Token::Dot);
    }

    #[test]
    fn test_operators() {
        assert_eq!(
            scan_single_token("+").unwrap(),
            Token::Operator("+".to_string())
        );
        assert_eq!(
            scan_single_token("==").unwrap(),
            Token::Operator("==".to_string())
        );
        assert_eq!(
            scan_single_token("<=").unwrap(),
            Token::Operator("<=".to_string())
        );
        assert_eq!(
            scan_single_token("&&").unwrap(),
            Token::Operator("&&".to_string())
        );
    }

    #[test]
    fn test_error_reporting() {
        let mut scanner = Scanner::new("\"unterminated", "test.jsonnet");
        let error = scanner.scan_next().unwrap_err();
        assert_eq!(error.source_id, "test.jsonnet");

        let report = error.into_report();
        // Test that report can be created without panicking
        let _ = report;
    }

    #[test]
    fn test_forbidden_operator_sequences() {
        let mut scanner = Scanner::new("//", "test");
        // Should not tokenize as operator since // starts a comment
        scanner.skip_whitespace_and_comments().unwrap();
        assert!(scanner.is_at_end());
    }

    #[test]
    fn test_trailing_semicolon_error() {
        let mut scanner = Scanner::new("true;", "test");
        let result = scanner.scan_all();
        assert!(result.is_err());

        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("Trailing semicolon"));
    }

    #[test]
    fn test_valid_semicolon_in_middle() {
        let mut scanner = Scanner::new("local x = 1; x", "test");
        let result = scanner.scan_all();
        assert!(result.is_ok());
    }
}
