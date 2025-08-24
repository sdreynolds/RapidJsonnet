use std::fs;
use std::ops::Range;
use chunk::{Chunk, Opcode, Value};
use scanner::{Scanner, ScanError, Token, TokenInfo};
use parser::Parser;
use string_pool::{InternedString, intern_string};

pub type CompilerError = ScanError;

// Expression type tracking for compile-time optimizations
#[derive(Debug, Clone, PartialEq)]
enum ExpressionType {
    String,
    Number,
    Boolean,
    Null,
    Unknown,
}

// Precedence constants
const PRECEDENCE_POSTFIX: u8 = 90;
const PRECEDENCE_UNARY: u8 = 80;
const PRECEDENCE_EXPONENTIATION: u8 = 70;
const PRECEDENCE_MULTIPLICATIVE: u8 = 60;
const PRECEDENCE_ADDITIVE: u8 = 50;
const PRECEDENCE_COMPARISON: u8 = 40;
const PRECEDENCE_EQUALITY: u8 = 35;
const PRECEDENCE_BITAND: u8 = 30;
const PRECEDENCE_BITXOR: u8 = 25;
const PRECEDENCE_BITOR: u8 = 20;
const PRECEDENCE_LOGICAL_AND: u8 = 15;
const PRECEDENCE_LOGICAL_OR: u8 = 10;
const PRECEDENCE_TERNARY: u8 = 5;

pub struct Compiler<'a> {
    compiling_chunk: Chunk<'a>,
    parser: Parser<'a>,
    type_stack: Vec<ExpressionType>,
}

impl<'a> Compiler<'a> {
    pub fn new(scanner: &'a mut Scanner<'a>, source_id: &'a str) -> Self {
        let parser = Parser::new(scanner);
        let compiling_chunk = Chunk::new(source_id);

        Self {
            compiling_chunk,
            parser,
            type_stack: Vec::new(),
        }
    }

    pub fn new_from_file(file_name: &'static str) -> Result<Compiler<'static>, std::io::Error> {
        let contents = fs::read_to_string(file_name)?;
        // We need to leak the string to get a 'static lifetime for the compiler
        let input: &'static str = Box::leak(contents.into_boxed_str());
        let scanner: &'static mut Scanner = Box::leak(Box::new(Scanner::new(input, file_name)));
        Ok(Compiler::new(scanner, file_name))
    }

    pub fn compile(mut self) -> Result<Chunk<'a>, CompilerError> {
        // Advance to get the first token
        self.parser.advance()?;

        // Parse the entire expression
        self.parse_expr(0)?;

        // Validate that no unexpected tokens remain after parsing the expression
        self.check_end_of_input()?;

        // Emit return opcode at the end - use the span of the last token or end of input
        let span = self.current_span();
        self.emit_opcode(Opcode::Return, span);

        Ok(self.compiling_chunk)
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<(), CompilerError> {
        // Parse left-hand side (prefix)
        self.parse_prefix()?;

        // Parse infix operators
        loop {
            // Check if we're at the end or if there's no current token
            if self.parser.is_at_end() {
                break;
            }

            let current = match self.parser.current_token() {
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
        let token = self.parser.current_token().cloned()
            .ok_or_else(|| {
                // Use the previous token's span end as the EOF location, or 0..0 if no previous token
                let span = if let Some(previous) = self.parser.previous_token() {
                    previous.span.end..previous.span.end
                } else {
                    0..0
                };
                self.unexpected_eof_error(span)
            })?;

        match &token.token {
            Token::Number(value) => {
                self.emit_constant(*value)?;
                self.push_type(ExpressionType::Number);
                self.parser.advance()?; // consume the number
            }
            Token::String(value) => {
                self.emit_string_constant(*value)?;
                self.push_type(ExpressionType::String);
                self.parser.advance()?; // consume the string
            }
            Token::True => {
                self.emit_opcode(Opcode::LoadTrue, token.span);
                self.push_type(ExpressionType::Boolean);
                self.parser.advance()?; // consume the true
            }
            Token::False => {
                self.emit_opcode(Opcode::LoadFalse, token.span);
                self.push_type(ExpressionType::Boolean);
                self.parser.advance()?; // consume the false
            }
            Token::Null => {
                self.emit_opcode(Opcode::LoadNull, token.span);
                self.push_type(ExpressionType::Null);
                self.parser.advance()?; // consume the null
            }
            Token::Operator(op) if op == "-" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Neg, token.span);
                // Unary minus: if operand was number, result is number; otherwise unknown
                let operand_type = self.pop_type();
                if operand_type == ExpressionType::Number {
                    self.push_type(ExpressionType::Number);
                } else {
                    self.push_type(ExpressionType::Unknown);
                }
            }
            Token::Operator(op) if op == "+" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Pos, token.span);
                // Unary plus: if operand was number, result is number; otherwise unknown
                let operand_type = self.pop_type();
                if operand_type == ExpressionType::Number {
                    self.push_type(ExpressionType::Number);
                } else {
                    self.push_type(ExpressionType::Unknown);
                }
            }
            Token::Operator(op) if op == "!" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::Not, token.span);
                // Logical NOT always produces boolean
                self.pop_type();
                self.push_type(ExpressionType::Boolean);
            }
            Token::Operator(op) if op == "~" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr(PRECEDENCE_UNARY)?;
                self.emit_opcode(Opcode::BitNot, token.span);
                // Bitwise NOT: if operand was number, result is number; otherwise unknown
                let operand_type = self.pop_type();
                if operand_type == ExpressionType::Number {
                    self.push_type(ExpressionType::Number);
                } else {
                    self.push_type(ExpressionType::Unknown);
                }
            }
            Token::LeftParen => {
                self.parser.advance()?; // consume '('
                self.parse_expr(0)?;
                self.parser.consume(Token::RightParen, "Expected closing parenthesis")?;
                // Parentheses don't change the type
            }
            _ => {
                // Unknown expression type
                self.push_type(ExpressionType::Unknown);
                return Err(self.invalid_expression_error(&token));
            }
        }

        Ok(())
    }

    fn parse_infix(&mut self, left_bp: u8) -> Result<(), CompilerError> {
        let token = self.parser.current_token().cloned()
            .ok_or_else(|| {
                // Use the previous token's span end as the EOF location
                let span = if let Some(previous) = self.parser.previous_token() {
                    previous.span.end..previous.span.end
                } else {
                    0..0
                };
                self.unexpected_eof_error(span)
            })?;

        self.parser.advance()?; // consume operator
        self.parse_expr(left_bp)?;

        match &token.token {
            Token::Operator(op) if op == "+" => {
                // Check operand types for optimization
                let right_type = self.pop_type();
                let left_type = self.pop_type();
                
                // If both operands are known to be strings, use StringConcat
                if left_type == ExpressionType::String && right_type == ExpressionType::String {
                    self.emit_opcode(Opcode::StringConcat, token.span);
                    self.push_type(ExpressionType::String);
                } else {
                    // Fall back to Add for mixed/unknown types
                    self.emit_opcode(Opcode::Add, token.span);
                    
                    // Determine result type
                    if left_type == ExpressionType::String || right_type == ExpressionType::String {
                        self.push_type(ExpressionType::String);
                    } else if left_type == ExpressionType::Number && right_type == ExpressionType::Number {
                        self.push_type(ExpressionType::Number);
                    } else {
                        self.push_type(ExpressionType::Unknown);
                    }
                }
            }
            Token::Operator(op) if op == "-" => {
                self.emit_opcode(Opcode::Sub, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // subtraction always produces number
            }
            Token::Operator(op) if op == "*" => {
                self.emit_opcode(Opcode::Mul, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // multiplication always produces number
            }
            Token::Operator(op) if op == "/" => {
                self.emit_opcode(Opcode::Div, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // division always produces number
            }
            Token::Operator(op) if op == "<" => {
                self.emit_opcode(Opcode::Lt, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // comparison always produces boolean
            }
            Token::Operator(op) if op == "<=" => {
                self.emit_opcode(Opcode::Le, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // comparison always produces boolean
            }
            Token::Operator(op) if op == ">" => {
                self.emit_opcode(Opcode::Gt, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // comparison always produces boolean
            }
            Token::Operator(op) if op == ">=" => {
                self.emit_opcode(Opcode::Ge, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // comparison always produces boolean
            }
            Token::Operator(op) if op == "==" => {
                self.emit_opcode(Opcode::Eq, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // equality always produces boolean
            }
            Token::Operator(op) if op == "!=" => {
                self.emit_opcode(Opcode::Ne, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Boolean); // inequality always produces boolean
            }
            Token::Operator(op) if op == "&" => {
                self.emit_opcode(Opcode::BitAnd, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // bitwise ops produce numbers
            }
            Token::Operator(op) if op == "|" => {
                self.emit_opcode(Opcode::BitOr, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // bitwise ops produce numbers
            }
            _ => return Err(self.invalid_expression_error(&token)),
        }

        Ok(())
    }


    fn emit_opcode(&mut self, opcode: Opcode, span: Range<usize>) {
        self.compiling_chunk.write_opcode(opcode, span);
    }

    fn emit_constant(&mut self, value: f64) -> Result<u16, CompilerError> {
        let index = self.compiling_chunk.add_constant(Value::Number(value));
        if index > u16::MAX as usize {
            return Err(self.too_many_constants_error());
        }

        // Use the current token's span for the constant
        let span = self.current_span();

        self.compiling_chunk.write_opcode_u16(Opcode::LoadConst, index as u16, span);
        Ok(index as u16)
    }

    fn emit_string_constant(&mut self, value: InternedString) -> Result<u16, CompilerError> {
        let index = self.compiling_chunk.add_constant(Value::String(value));
        if index > u16::MAX as usize {
            return Err(self.too_many_constants_error());
        }

        // Use the current token's span for the constant
        let span = self.current_span();

        self.compiling_chunk.write_opcode_u16(Opcode::LoadConst, index as u16, span);
        Ok(index as u16)
    }

    fn current_span(&self) -> Range<usize> {
        if let Some(current) = self.parser.current_token() {
            current.span.clone()
        } else if let Some(previous) = self.parser.previous_token() {
            previous.span.clone()
        } else {
            0..0
        }
    }

    fn make_error(&self, span: Range<usize>, message: String) -> CompilerError {
        ScanError {
            span,
            message,
            source_id: self.parser.source_id().to_string(),
        }
    }

    // Type stack management for compile-time optimizations
    fn push_type(&mut self, expr_type: ExpressionType) {
        self.type_stack.push(expr_type);
    }

    fn pop_type(&mut self) -> ExpressionType {
        self.type_stack.pop().unwrap_or(ExpressionType::Unknown)
    }

    fn peek_type(&self) -> ExpressionType {
        self.type_stack.last().cloned().unwrap_or(ExpressionType::Unknown)
    }

    fn unexpected_eof_error(&self, span: Range<usize>) -> CompilerError {
        self.make_error(span, "Unexpected end of input".to_string())
    }

    fn invalid_expression_error(&self, token: &TokenInfo) -> CompilerError {
        self.make_error(
            token.span.clone(),
            format!("Invalid expression starting with {:?}", token.token)
        )
    }

    fn too_many_constants_error(&self) -> CompilerError {
        self.make_error(
            0..0, // This error doesn't relate to a specific token location
            "Too many constants (maximum 65535)".to_string()
        )
    }

    fn unexpected_token_after_expression_error(&self, token: &TokenInfo) -> CompilerError {
        let message = match &token.token {
            Token::RightParen => "Unexpected ')' - no matching opening parenthesis".to_string(),
            Token::RightBrace => "Unexpected '}' - no matching opening brace".to_string(),
            Token::RightBracket => "Unexpected ']' - no matching opening bracket".to_string(),
            _ => format!("Unexpected token {:?} after complete expression", token.token),
        };
        self.make_error(token.span.clone(), message)
    }

    fn check_end_of_input(&self) -> Result<(), CompilerError> {
        if !self.parser.is_at_end() {
            if let Some(current) = self.parser.current_token() {
                return Err(self.unexpected_token_after_expression_error(current));
            }
        }
        Ok(())
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

            // Equality (left associative)
            Token::Operator(op) if matches!(op.as_str(), "==" | "!=") =>
                Some((PRECEDENCE_EQUALITY, PRECEDENCE_EQUALITY + 1)),

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
        let mut scanner = Scanner::new("42", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::Number(42.0));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_simple_addition() {
        let mut scanner = Scanner::new("3 + 4", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], Value::Number(3.0));
        assert_eq!(chunk.constants[1], Value::Number(4.0));
        // LoadConst (3) + LoadConst (3) + Add (1) + Return (1) = 8 bytes
        assert_eq!(chunk.code.len(), 8);
    }

    #[test]
    fn test_unary_minus() {
        let mut scanner = Scanner::new("-42", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::Number(42.0));
        // LoadConst (3) + Neg (1) + Return (1) = 5 bytes
        assert_eq!(chunk.code.len(), 5);
    }

    #[test]
    fn test_grouped_expression() {
        let mut scanner = Scanner::new("(1 + 2) * 3", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 3);
        // LoadConst (3) + LoadConst (3) + Add (1) + LoadConst (3) + Mul (1) + Return (1) = 12 bytes
        assert_eq!(chunk.code.len(), 12);
    }

    #[test]
    fn test_precedence() {
        let mut scanner = Scanner::new("2 + 3 * 4", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        // Should parse as 2 + (3 * 4), so constants should be in order 2, 3, 4
        assert_eq!(chunk.constants.len(), 3);
        assert_eq!(chunk.constants[0], Value::Number(2.0));
        assert_eq!(chunk.constants[1], Value::Number(3.0));
        assert_eq!(chunk.constants[2], Value::Number(4.0));
    }

    #[test]
    fn test_true_literal() {
        let mut scanner = Scanner::new("true", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadTrue (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadTrue as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_false_literal() {
        let mut scanner = Scanner::new("false", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadFalse (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadFalse as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_null_literal() {
        let mut scanner = Scanner::new("null", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadNull (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadNull as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_logical_not_with_true() {
        let mut scanner = Scanner::new("!true", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 3); // LoadTrue (1 byte) + Not (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadTrue as u8);
        assert_eq!(chunk.code[1], Opcode::Not as u8);
        assert_eq!(chunk.code[2], Opcode::Return as u8);
    }

    #[test]
    fn test_string_literal() {
        let mut scanner = Scanner::new("\"hello world\"", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::String(intern_string("hello world")));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_empty_string_literal() {
        let mut scanner = Scanner::new("\"\"", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::String(intern_string("")));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_string_with_logical_not() {
        let mut scanner = Scanner::new("!\"test\"", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::String(intern_string("test")));
        assert_eq!(chunk.code.len(), 5); // LoadConst (3 bytes) + Not (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadConst as u8);
        assert_eq!(chunk.code[3], Opcode::Not as u8);
        assert_eq!(chunk.code[4], Opcode::Return as u8);
    }
}
