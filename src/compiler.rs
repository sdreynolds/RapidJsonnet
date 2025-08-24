use std::fs;
use std::ops::Range;
use chunk::{Chunk, Opcode};
use scanner::{Scanner, ScanError, Token, TokenInfo};

pub type CompilerError = ScanError;

// Helper functions for creating compiler errors
pub fn unexpected_token(token: &TokenInfo, expected: &str) -> CompilerError {
    ScanError {
        span: token.span.clone(),
        message: format!("Expected {}, found {:?}", expected, token.token),
        source_id: "compiler".to_string(),
    }
}

pub fn unexpected_eof(span: Range<usize>) -> CompilerError {
    ScanError {
        span,
        message: "Unexpected end of input".to_string(),
        source_id: "compiler".to_string(),
    }
}

pub fn invalid_expression(token: &TokenInfo) -> CompilerError {
    ScanError {
        span: token.span.clone(),
        message: format!("Invalid expression starting with {:?}", token.token),
        source_id: "compiler".to_string(),
    }
}

pub fn too_many_constants() -> CompilerError {
    ScanError {
        span: 0..0,
        message: "Too many constants (maximum 65535)".to_string(),
        source_id: "compiler".to_string(),
    }
}

// Precedence constants
const PRECEDENCE_POSTFIX: u8 = 90;
const PRECEDENCE_UNARY: u8 = 80;
const PRECEDENCE_EXPONENTIATION: u8 = 70;
const PRECEDENCE_MULTIPLICATIVE: u8 = 60;
const PRECEDENCE_ADDITIVE: u8 = 50;
const PRECEDENCE_COMPARISON: u8 = 40;
const PRECEDENCE_BITAND: u8 = 30;
const PRECEDENCE_BITXOR: u8 = 25;
const PRECEDENCE_BITOR: u8 = 20;
const PRECEDENCE_LOGICAL_AND: u8 = 15;
const PRECEDENCE_LOGICAL_OR: u8 = 10;
const PRECEDENCE_TERNARY: u8 = 5;

pub struct Compiler<'a> {
    compiling_chunk: Chunk<'a>,
    scanner: Scanner<'a>,
    current_token: Option<TokenInfo>,
}

impl<'a> Compiler<'a> {
    pub fn new(input: &'a str, source_id: &'a str) -> Self {
        let scanner = Scanner::new(input, source_id);
        let compiling_chunk = Chunk::new(source_id);
        
        Self {
            compiling_chunk,
            scanner,
            current_token: None,
        }
    }

    pub fn new_from_file(file_name: &'static str) -> Result<Compiler<'static>, std::io::Error> {
        let contents = fs::read_to_string(file_name)?;
        // We need to leak the string to get a 'static lifetime for the compiler
        let input: &'static str = Box::leak(contents.into_boxed_str());
        Ok(Compiler::new(input, file_name))
    }

    pub fn compile(mut self) -> Result<Chunk<'a>, CompilerError> {
        // Parse the entire expression
        self.parse_expr(0)?;
        
        // Emit return opcode at the end
        let span = 0..0; // Simple span for now
        self.emit_opcode(Opcode::Return, span);
        
        Ok(self.compiling_chunk)
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<(), CompilerError> {
        // Parse left-hand side (prefix)
        self.parse_prefix()?;
        
        // Parse infix operators
        loop {
            let current = match self.peek_token()? {
                Some(token) => token,
                None => break,
            };
            
            if let Some((left_bp, right_bp)) = self.get_binding_power(&current.token) {
                if left_bp <= min_bp {
                    break;
                }
                
                self.parse_infix(right_bp)?;
            } else {
                break;
            }
        }
        
        Ok(())
    }

    fn parse_prefix(&mut self) -> Result<(), CompilerError> {
        let token = self.advance_token()?;
        
        match &token.token {
            Token::Number(value) => {
                self.emit_constant(*value)?;
            }
            Token::Operator(op) if op == "-" => {
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Neg, token.span);
            }
            Token::Operator(op) if op == "+" => {
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Pos, token.span);
            }
            Token::Operator(op) if op == "!" => {
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Not, token.span);
            }
            Token::Operator(op) if op == "~" => {
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::BitNot, token.span);
            }
            Token::LeftParen => {
                self.parse_expr(0)?;
                
                let current = self.advance_token()?;
                if current.token != Token::RightParen {
                    return Err(unexpected_token(&current, "closing parenthesis"));
                }
            }
            _ => {
                return Err(invalid_expression(&token));
            }
        }
        
        Ok(())
    }

    fn parse_infix(&mut self, left_bp: u8) -> Result<(), CompilerError> {
        let token = self.advance_token()?; // consume operator
        self.parse_expr(left_bp)?;
        
        match &token.token {
            Token::Operator(op) if op == "+" => self.emit_opcode(Opcode::Add, token.span),
            Token::Operator(op) if op == "-" => self.emit_opcode(Opcode::Sub, token.span),
            Token::Operator(op) if op == "*" => self.emit_opcode(Opcode::Mul, token.span),
            Token::Operator(op) if op == "/" => self.emit_opcode(Opcode::Div, token.span),
            Token::Operator(op) if op == "<" => self.emit_opcode(Opcode::Lt, token.span),
            Token::Operator(op) if op == "<=" => self.emit_opcode(Opcode::Le, token.span),
            Token::Operator(op) if op == ">" => self.emit_opcode(Opcode::Gt, token.span),
            Token::Operator(op) if op == ">=" => self.emit_opcode(Opcode::Ge, token.span),
            Token::Operator(op) if op == "&" => self.emit_opcode(Opcode::BitAnd, token.span),
            Token::Operator(op) if op == "|" => self.emit_opcode(Opcode::BitOr, token.span),
            _ => return Err(invalid_expression(&token)),
        }
        
        Ok(())
    }

    fn advance_token(&mut self) -> Result<TokenInfo, CompilerError> {
        if let Some(token) = self.current_token.take() {
            Ok(token)
        } else {
            self.scanner.scan_next()
        }
    }
    
    fn peek_token(&mut self) -> Result<Option<TokenInfo>, CompilerError> {
        if self.current_token.is_none() {
            match self.scanner.scan_next() {
                Ok(token) if matches!(token.token, Token::Eof) => return Ok(None),
                Ok(token) => self.current_token = Some(token),
                Err(e) => return Err(e),
            }
        }
        
        Ok(self.current_token.clone())
    }

    fn emit_opcode(&mut self, opcode: Opcode, span: Range<usize>) {
        self.compiling_chunk.write_opcode(opcode, span);
    }

    fn emit_constant(&mut self, value: f64) -> Result<u16, CompilerError> {
        let index = self.compiling_chunk.add_constant(value);
        if index > u16::MAX as usize {
            return Err(too_many_constants());
        }
        
        let span = 0..0; // Simple span for now
        self.compiling_chunk.write_opcode_u16(Opcode::LoadConst, index as u16, span);
        Ok(index as u16)
    }

    fn get_binding_power(&self, token: &Token) -> Option<(u8, u8)> {
        match token {
            // Multiplicative (left associative)
            Token::Operator(op) if op == "*" || op == "/" => 
                Some((PRECEDENCE_MULTIPLICATIVE, PRECEDENCE_MULTIPLICATIVE + 1)),
            
            // Additive (left associative)
            Token::Operator(op) if op == "+" || op == "-" => 
                Some((PRECEDENCE_ADDITIVE, PRECEDENCE_ADDITIVE + 1)),
            
            // Comparison (non-associative)
            Token::Operator(op) if matches!(op.as_str(), "<" | "<=" | ">" | ">=") => 
                Some((PRECEDENCE_COMPARISON, PRECEDENCE_COMPARISON + 1)),
            
            // Bitwise AND (left associative)
            Token::Operator(op) if op == "&" => 
                Some((PRECEDENCE_BITAND, PRECEDENCE_BITAND + 1)),
            
            // Bitwise OR (left associative)
            Token::Operator(op) if op == "|" => 
                Some((PRECEDENCE_BITOR, PRECEDENCE_BITOR + 1)),
            
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_number() {
        let compiler = Compiler::new("42", "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_simple_addition() {
        let compiler = Compiler::new("3 + 4", "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], 3.0);
        assert_eq!(chunk.constants[1], 4.0);
        // LoadConst (3) + LoadConst (3) + Add (1) + Return (1) = 8 bytes
        assert_eq!(chunk.code.len(), 8);
    }

    #[test]
    fn test_unary_minus() {
        let compiler = Compiler::new("-42", "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
        // LoadConst (3) + Neg (1) + Return (1) = 5 bytes
        assert_eq!(chunk.code.len(), 5);
    }

    #[test]
    fn test_grouped_expression() {
        let compiler = Compiler::new("(1 + 2) * 3", "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 3);
        // LoadConst (3) + LoadConst (3) + Add (1) + LoadConst (3) + Mul (1) + Return (1) = 12 bytes
        assert_eq!(chunk.code.len(), 12);
    }

    #[test]
    fn test_precedence() {
        let compiler = Compiler::new("2 + 3 * 4", "test");
        let chunk = compiler.compile().unwrap();
        
        // Should parse as 2 + (3 * 4), so constants should be in order 2, 3, 4
        assert_eq!(chunk.constants.len(), 3);
        assert_eq!(chunk.constants[0], 2.0);
        assert_eq!(chunk.constants[1], 3.0);
        assert_eq!(chunk.constants[2], 4.0);
    }
}