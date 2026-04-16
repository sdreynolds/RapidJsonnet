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
pub struct ScannerCheckpoint {
    pub position: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub struct ScanError {
    pub span: Range<usize>,
    pub message: String,
    pub source_id: String,
    pub cause: Option<Box<ScanError>>,
}

impl ScanError {
    pub fn new(span: Range<usize>, message: String, source_id: String) -> Self {
        Self {
            span,
            message,
            source_id,
            cause: None,
        }
    }

    pub fn is_incomplete_input(&self) -> bool {
        let msg = self.message.to_lowercase();
        msg.contains("unexpected end of input")
            || msg.contains("unterminated")
            || msg.contains("starting with eof")
            || msg.contains("found eof")
    }

    pub fn into_report(&self) -> (Report<'static, (String, Range<usize>)>, Vec<String>) {
        let red = ariadne::Color::Red;
        let yellow = ariadne::Color::Yellow;

        // Collect all errors in the chain (outermost first, root cause last)
        let mut chain: Vec<&ScanError> = Vec::new();
        let mut current = self;
        loop {
            chain.push(current);
            match &current.cause {
                Some(next) => current = next,
                None => break,
            }
        }

        // Root cause is the last element
        let root_cause = chain.last().unwrap();

        // Build report with root cause as primary
        let mut builder = Report::build(
            ReportKind::Error,
            (root_cause.source_id.clone(), root_cause.span.clone()),
        )
        .with_message(&root_cause.message)
        .with_label(
            Label::new((root_cause.source_id.clone(), root_cause.span.clone()))
                .with_message(&root_cause.message)
                .with_color(red),
        );

        // Add caller frames as additional labels (all except the last/root cause)
        for frame in chain.iter().take(chain.len().saturating_sub(1)) {
            builder = builder.with_label(
                Label::new((frame.source_id.clone(), frame.span.clone()))
                    .with_message(&frame.message)
                    .with_color(yellow),
            );
        }

        // Collect unique source IDs
        let mut source_ids: Vec<String> = Vec::new();
        for frame in &chain {
            if !source_ids.contains(&frame.source_id) {
                source_ids.push(frame.source_id.clone());
            }
        }

        (builder.finish(), source_ids)
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

    /// Save scanner position for backtracking
    pub fn save_position(&self) -> ScannerCheckpoint {
        ScannerCheckpoint {
            position: self.position,
            line: self.line,
            column: self.column,
        }
    }

    /// Restore scanner to a previously saved position
    pub fn restore_position(&mut self, checkpoint: ScannerCheckpoint) {
        self.position = checkpoint.position;
        self.line = checkpoint.line;
        self.column = checkpoint.column;
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
        ScanError::new(span, message, self.source_id.to_string())
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
                                    // Check for UTF-16 surrogate pair (high surrogate: D800-DBFF)
                                    if (0xD800..=0xDBFF).contains(&code_point) {
                                        // Expect \uXXXX low surrogate (DC00-DFFF)
                                        if self.peek() == '\\' && self.peek_ahead(1) == Some('u') {
                                            self.advance(); // consume '\'
                                            self.advance(); // consume 'u'
                                            let low_hex: String =
                                                (0..4).map(|_| self.advance()).collect();
                                            if let Ok(low_cp) = u32::from_str_radix(&low_hex, 16) {
                                                if (0xDC00..=0xDFFF).contains(&low_cp) {
                                                    let full_cp = 0x10000
                                                        + ((code_point - 0xD800) << 10)
                                                        + (low_cp - 0xDC00);
                                                    if let Some(ch) = char::from_u32(full_cp) {
                                                        value.push(ch);
                                                    } else {
                                                        return Err(self.make_error(
                                                            hex_start - 2..self.position,
                                                            format!(
                                                                "Invalid surrogate pair: \\u{}\\u{}",
                                                                hex_digits, low_hex
                                                            ),
                                                        ));
                                                    }
                                                } else {
                                                    return Err(self.make_error(
                                                        hex_start - 2..self.position,
                                                        format!(
                                                            "Expected low surrogate after \\u{}, got \\u{}",
                                                            hex_digits, low_hex
                                                        ),
                                                    ));
                                                }
                                            } else {
                                                return Err(self.make_error(
                                                    hex_start - 2..self.position,
                                                    format!(
                                                        "Invalid unicode escape after surrogate: \\u{}",
                                                        low_hex
                                                    ),
                                                ));
                                            }
                                        } else {
                                            return Err(self.make_error(
                                                hex_start - 2..self.position,
                                                format!(
                                                    "High surrogate \\u{} not followed by low surrogate",
                                                    hex_digits
                                                ),
                                            ));
                                        }
                                    } else if let Some(unicode_char) = char::from_u32(code_point) {
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

        // Collect raw lines and find the closing |||.
        let mut raw_lines: Vec<String> = Vec::new();

        loop {
            if self.is_at_end() {
                return Err(
                    self.make_error(start..self.position, "Unterminated text block".to_string())
                );
            }

            let line_start = self.position;
            let mut line = String::new();

            while !self.is_at_end() && self.peek() != '\n' {
                line.push(self.advance());
            }

            if !self.is_at_end() {
                self.advance(); // consume newline
            }

            // Check if this line contains the closing |||
            let trimmed = line.trim_start();
            if trimmed.starts_with("|||") {
                // Position scanner right after the ||| so tokens after it
                // (like ;) can be scanned normally.
                let leading_ws = line.len() - trimmed.len();
                self.position = line_start + leading_ws + 3;
                break;
            }

            raw_lines.push(line);
        }

        // Determine indentation from the first non-blank content line
        let indent = raw_lines
            .iter()
            .find(|l| !l.trim().is_empty())
            .map(|first_line| {
                let trimmed = first_line.trim_start();
                first_line[..first_line.len() - trimmed.len()].to_string()
            })
            .unwrap_or_default();

        // Strip the base indentation from each content line
        let mut lines: Vec<String> = Vec::new();
        for raw_line in &raw_lines {
            if raw_line.trim().is_empty() {
                lines.push(String::new());
            } else if raw_line.starts_with(&indent) {
                lines.push(raw_line[indent.len()..].to_string());
            } else {
                return Err(self.make_error(
                    start..self.position,
                    format!(
                        "Text block line doesn't start with expected indentation: '{}'",
                        indent.escape_default()
                    ),
                ));
            }
        }

        // Build result: join with newlines, add trailing newline
        let mut result = lines.join("\n");
        if !result.is_empty() {
            result.push('\n');
        }
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

            // Don't consume '|' if it starts a ||| text block
            if ch == '|' && self.peek_ahead(1) == Some('|') && self.peek_ahead(2) == Some('|') {
                break;
            }

            // Don't consume '/' if it starts a comment (// or /*)
            if ch == '/' && (self.peek_ahead(1) == Some('/') || self.peek_ahead(1) == Some('*')) {
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

            // Don't extend '+' with ':' — '+:' is field override syntax, not an operator
            if operator == "+" && ch == ':' {
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

        let (report, source_ids) = error.into_report();
        // Test that report can be created without panicking
        let _ = report;
        assert!(!source_ids.is_empty());
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

    fn scan_all_ok(input: &str) -> Vec<Token> {
        let mut scanner = Scanner::new(input, "test");
        scanner
            .scan_all()
            .unwrap()
            .into_iter()
            .map(|t| t.token)
            .collect()
    }

    fn scan_all_err(input: &str) -> Vec<ScanError> {
        let mut scanner = Scanner::new(input, "test");
        scanner.scan_all().unwrap_err()
    }

    #[test]
    fn test_text_block_basic() {
        let input = "|||\n  hello\n  world\n|||";
        let tokens = scan_all_ok(input);
        assert!(matches!(&tokens[0], Token::String(s) if s == "hello\nworld\n"));
    }

    #[test]
    fn test_text_block_strip_newline() {
        let input = "|||-\n  hello\n|||";
        let tokens = scan_all_ok(input);
        assert!(matches!(&tokens[0], Token::String(s) if s == "hello"));
    }

    #[test]
    fn test_text_block_missing_newline_error() {
        let mut scanner = Scanner::new("|||   no-newline", "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_text_block_unterminated_error() {
        let mut scanner = Scanner::new("|||\n  hello\n", "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_text_block_mismatched_indent_error() {
        let input = "|||\n  good\n bad\n|||";
        let mut scanner = Scanner::new(input, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_verbatim_string_double_quote_escape() {
        let tokens = scan_all_ok(r#"@"foo""bar""#);
        assert!(matches!(&tokens[0], Token::String(s) if s == r#"foo"bar"#));
    }

    #[test]
    fn test_verbatim_string_single_quote_escape() {
        let tokens = scan_all_ok("@'it''s'");
        assert!(matches!(&tokens[0], Token::String(s) if s == "it's"));
    }

    #[test]
    fn test_unicode_escape_basic() {
        let tokens = scan_all_ok(r#""\u0041""#); // A
        assert!(matches!(&tokens[0], Token::String(s) if s == "A"));
    }

    #[test]
    fn test_unicode_surrogate_pair() {
        // U+1F600 GRINNING FACE = \uD83D\uDE00
        let tokens = scan_all_ok(r#""\uD83D\uDE00""#);
        assert!(matches!(&tokens[0], Token::String(s) if s == "\u{1F600}"));
    }

    #[test]
    fn test_unicode_unpaired_high_surrogate_error() {
        let mut scanner = Scanner::new(r#""\uD83D""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_unicode_invalid_low_surrogate_error() {
        let mut scanner = Scanner::new(r#""\uD83D\u0041""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_block_comment_unterminated() {
        let errors = scan_all_err("/* unterminated");
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated"));
    }

    #[test]
    fn test_is_incomplete_input_true() {
        let err = ScanError::new(
            0..1,
            "Unexpected end of input".to_string(),
            "test".to_string(),
        );
        assert!(err.is_incomplete_input());

        let err2 = ScanError::new(0..1, "Unterminated string".to_string(), "test".to_string());
        assert!(err2.is_incomplete_input());
    }

    #[test]
    fn test_is_incomplete_input_false() {
        let err = ScanError::new(
            0..1,
            "Unexpected character 'x'".to_string(),
            "test".to_string(),
        );
        assert!(!err.is_incomplete_input());
    }

    #[test]
    fn test_into_report_with_cause() {
        let cause = ScanError::new(0..3, "root cause".to_string(), "test".to_string());
        let mut outer = ScanError::new(5..8, "outer error".to_string(), "test".to_string());
        outer.cause = Some(Box::new(cause));

        let (report, source_ids) = outer.into_report();
        let _ = report; // just verify it doesn't panic
        assert_eq!(source_ids.len(), 1);
    }

    #[test]
    fn test_save_and_restore_position() {
        let mut scanner = Scanner::new("foo bar", "test");
        // Consume "foo" token to advance past it
        scanner.scan_next().unwrap();
        let checkpoint = scanner.save_position();
        // Consume "bar" token
        scanner.scan_next().unwrap();
        scanner.restore_position(checkpoint);
        // Should be back at "bar"
        let result = scanner.scan_next().unwrap();
        assert!(matches!(result.token, Token::Identifier(s) if s == "bar"));
    }

    #[test]
    fn test_collected_strings_after_scan() {
        let mut scanner = Scanner::new(r#""hello" "world""#, "test");
        scanner.scan_all().unwrap();
        assert_eq!(scanner.collected_strings().len(), 2);
    }

    #[test]
    fn test_invalid_escape_sequence() {
        let mut scanner = Scanner::new(r#""\q""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Invalid escape"));
    }

    #[test]
    fn test_number_invalid_exponent() {
        let errors = scan_all_err("1e");
        assert!(!errors.is_empty());
    }
}
