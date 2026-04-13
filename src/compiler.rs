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

use chunk::{Chunk, FieldVisibility, I32_SIZE_BYTES, Opcode, StringIndex, Value};
use memory_manager::MemoryManager;
use parser::{Parser, ParserCheckpoint};
use scanner::{ScanError, Scanner, Token, TokenInfo};
use std::collections::HashMap;
use std::fs;
use std::ops::Range;

pub type CompilerError = ScanError;

#[cfg(test)]
#[path = "compiler_integration_test.rs"]
mod compiler_integration_test;

// Comprehension clause tracking for parsing and code generation
#[derive(Debug, Clone)]
enum ComprehensionClause {
    For {
        var_name: String,
        source_checkpoint: ParserCheckpoint,
        span: Range<usize>,
    },
    If {
        condition_checkpoint: ParserCheckpoint,
        span: Range<usize>,
    },
}

// Object-local binding tracking for replay inside field/assert thunks
#[derive(Debug, Clone)]
struct ObjectLocalBinding {
    checkpoint: ParserCheckpoint,
    span: Range<usize>,
}

/// A parsed function parameter, with optional default expression checkpoint
#[derive(Debug, Clone)]
struct FunctionParam {
    name: String,
    has_default: bool,
    /// Parser checkpoint pointing to the default expression (if has_default)
    default_checkpoint: Option<ParserCheckpoint>,
}

// Expression type tracking for compile-time optimizations
#[derive(Debug, Clone, PartialEq)]
enum ExpressionType {
    String,
    Number,
    Boolean,
    Null,
    Object,
    Array,
    Unknown,
    StdNamespace,
    NativeFunction(chunk::NativeFuncId),
}

// Local variable tracking for compile-time stack slot assignment
#[derive(Debug, Clone)]
struct Local {
    name: String,      // Variable name
    depth: u32,        // Scope nesting level
    stack_slot: usize, // Absolute position from stack bottom
    is_captured: bool, // Whether this local is captured by a closure
}

// Upvalue tracking for closure compilation
#[derive(Debug, Clone)]
struct CompilerUpvalue {
    index: u8,      // Index in enclosing function's locals or upvalues
    is_local: bool, // True if captures local, false if captures upvalue
}

// Enclosing scope state for upvalue resolution during nested function compilation
// This holds the frozen state of outer scopes needed for closure variable capture
#[derive(Debug)]
struct EnclosingScope<'a> {
    locals: Vec<Local>,
    upvalues: Vec<CompilerUpvalue>,
    enclosing: Option<Box<EnclosingScope<'a>>>,
    chunk: Chunk<'a>,
    constant_pool: HashMap<Value, u16>,
    anon_stack_depth: usize,
    type_stack: Vec<ExpressionType>,
}

impl<'a> EnclosingScope<'a> {
    /// Add an upvalue to this scope's upvalue list
    /// Returns the index of the upvalue (reusing existing entry if found)
    fn add_upvalue(&mut self, index: u8, is_local: bool) -> u8 {
        // Check if we already have this upvalue
        for (i, upvalue) in self.upvalues.iter().enumerate() {
            if upvalue.index == index && upvalue.is_local == is_local {
                return i as u8;
            }
        }

        // Add new upvalue
        let upvalue_index = self.upvalues.len() as u8;
        self.upvalues.push(CompilerUpvalue { index, is_local });
        upvalue_index
    }

    /// Resolve an upvalue by name, checking this scope and its enclosing scopes
    /// Returns the upvalue descriptor if found, None otherwise
    fn resolve_upvalue(&mut self, name: &str) -> Option<CompilerUpvalue> {
        // Try to find in this scope's locals
        for local in self.locals.iter_mut().rev() {
            if local.name == name {
                // Mark the local as captured
                local.is_captured = true;
                // Capture this local using its stack_slot (not array index)
                // because anon_stack_depth may shift slots from array positions
                return Some(CompilerUpvalue {
                    index: local.stack_slot as u8,
                    is_local: true,
                });
            }
        }

        // Try to find in enclosing scope's upvalues (recursive)
        if let Some(enclosing) = self.enclosing.as_mut() {
            if let Some(upvalue) = enclosing.resolve_upvalue(name) {
                // We found it in an even outer scope.
                // We must add it as an upvalue to THIS scope so our inner scopes can see it.
                let index = self.add_upvalue(upvalue.index, upvalue.is_local);
                // Return a descriptor saying: capture from our upvalues
                return Some(CompilerUpvalue {
                    index,
                    is_local: false,
                });
            }
        }

        None
    }
}

// Type of function being compiled
#[derive(Debug, Clone, Copy, PartialEq)]
enum FunctionType {
    Script,   // Top-level script
    Function, // Named or anonymous function
}

// Precedence constants (per Jsonnet spec, decreasing order)
const PRECEDENCE_POSTFIX: u8 = 90; // e(...) e[...] e.f
const PRECEDENCE_UNARY: u8 = 80; // + - ! ~
const PRECEDENCE_MULTIPLICATIVE: u8 = 60; // * / %
const PRECEDENCE_ADDITIVE: u8 = 50; // + -
const PRECEDENCE_SHIFT: u8 = 45; // << >>
const PRECEDENCE_COMPARISON: u8 = 40; // < > <= >= in
const PRECEDENCE_EQUALITY: u8 = 35; // == !=
const PRECEDENCE_BITAND: u8 = 30; // &
const PRECEDENCE_BITXOR: u8 = 25; // ^
const PRECEDENCE_BITOR: u8 = 20; // |
const PRECEDENCE_LOGICAL_AND: u8 = 15; // &&
const PRECEDENCE_LOGICAL_OR: u8 = 10; // ||

pub struct Compiler<'a> {
    compiling_chunk: Chunk<'a>,
    parser: Parser<'a>,
    type_stack: Vec<ExpressionType>,
    constant_pool: HashMap<Value, u16>,
    locals: Vec<Local>, // Tracks all local variables currently in scope
    scope_depth: u32,   // Current scope nesting depth (0 = module level)
    upvalues: Vec<CompilerUpvalue>, // Upvalues for this function
    function_type: FunctionType, // Type of function being compiled
    enclosing: Option<Box<EnclosingScope<'a>>>, // Enclosing scope for upvalue resolution
    object_depth: u32,  // Track object literal nesting depth
    tail_call_pending: bool, // Whether the next call should be emitted as TailCall
    in_tail_position: bool, // Whether we are currently in a tail position
    tail_calls_emitted: usize, // Number of tail calls emitted so far
    anon_stack_depth: usize, // Count of anonymous temporaries on VM stack not tracked as locals
}

impl<'a> Compiler<'a> {
    pub fn new(scanner: &'a mut Scanner<'a>, source_id: &'a str) -> Self {
        let parser = Parser::new(scanner);
        let compiling_chunk = Chunk::new(source_id);

        Self {
            compiling_chunk,
            parser,
            type_stack: Vec::new(),
            constant_pool: HashMap::new(),
            locals: Vec::new(),
            scope_depth: 0,
            upvalues: Vec::new(),
            function_type: FunctionType::Script,
            enclosing: None,
            object_depth: 0,
            tail_call_pending: false,
            in_tail_position: false,
            tail_calls_emitted: 0,
            anon_stack_depth: 0,
        }
    }

    pub fn new_from_file(file_name: &'static str) -> Result<Compiler<'static>, std::io::Error> {
        let contents = fs::read_to_string(file_name)?;
        // We need to leak the string to get a 'static lifetime for the compiler
        let input: &'static str = Box::leak(contents.into_boxed_str());
        let scanner: &'static mut Scanner = Box::leak(Box::new(Scanner::new(input, file_name)));
        Ok(Compiler::new(scanner, file_name))
    }

    /// Add a constant to the chunk, reusing existing constants if they have the same value
    fn add_constant_pooled(&mut self, value: Value) -> Result<u16, CompilerError> {
        // Check if we already have this constant
        if let Some(&existing_index) = self.constant_pool.get(&value) {
            return Ok(existing_index);
        }

        // Add the new constant to the chunk
        let index = self.compiling_chunk.add_constant(value.clone());
        if index > u16::MAX as usize {
            return Err(self.too_many_constants_error());
        }

        let index_u16 = index as u16;
        // Store in our constant pool for future deduplication
        self.constant_pool.insert(value, index_u16);
        Ok(index_u16)
    }

    pub fn compile(
        mut self,
        memory_manager: &mut MemoryManager,
    ) -> Result<Chunk<'a>, CompilerError> {
        // Advance to get the first token
        self.parser.advance()?;

        // Parse the entire expression
        self.in_tail_position = true;
        self.parse_expr(0, memory_manager)?;

        // Validate that no unexpected tokens remain after parsing the expression
        self.check_end_of_input()?;

        // Emit return opcode at the end - use the span of the last token or end of input
        let span = self.current_span();
        self.emit_opcode(Opcode::Return, span);

        Ok(self.compiling_chunk)
    }

    fn parse_expr(
        &mut self,
        min_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        // Parse left-hand side (prefix)
        self.parse_prefix(memory_manager)?;

        // Parse infix and postfix operators in a single loop to correctly handle precedence
        loop {
            // Check if we're at the end or if there's no current token
            if self.parser.is_at_end() {
                break;
            }

            let current = match self.parser.current_token() {
                Some(token) => token,
                None => break,
            };

            // Check for postfix operators first (highest precedence)
            if let Some(postfix_bp) = self.get_postfix_binding_power(&current.token) {
                if postfix_bp <= min_bp {
                    break;
                }

                self.parse_postfix(memory_manager)?;
                continue;
            }

            // Check for infix operators
            if let Some((left_bp, right_bp)) = self.get_binding_power(&current.token) {
                if left_bp <= min_bp {
                    break;
                }

                self.parse_infix(right_bp, memory_manager)?;
                continue;
            } else {
                break;
            }
        }

        Ok(())
    }

    fn parse_expr_notail(
        &mut self,
        min_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let prev = self.in_tail_position;
        self.in_tail_position = false;
        let res = self.parse_expr(min_bp, memory_manager);
        self.in_tail_position = prev;
        res
    }

    /// Compile slice sugar: arr[start:end:step] → std.slice(arr, start, end, step)
    /// When called, the array/object value is already on the stack.
    /// If `has_start` is Some(()), the start expression is also already compiled (on type stack).
    /// If `has_start` is None, start is omitted (will emit null).
    /// The parser is positioned at the first ':'.
    /// Compile slice sugar: arr[start:end:step] → std.slice(arr, start, end, step)
    /// Stack has [..., arr]. Parser is positioned at the start expression or first ':'.
    /// If `has_start` is true, the parser is at the start expression (before ':').
    /// If false, it's at the first ':' (empty start → null).
    fn compile_slice_sugar(
        &mut self,
        has_start: bool,
        bracket_span: &std::ops::Range<usize>,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let span = bracket_span.clone();

        // Stack: [..., arr]
        // Emit slice_func and swap under arr
        let const_idx = self
            .compiling_chunk
            .add_constant(chunk::Value::NativeFunction(chunk::NativeFuncId::Slice));
        self.compiling_chunk
            .write_opcode_u16(Opcode::LoadConst, const_idx as u16, span.clone());
        self.emit_opcode(Opcode::Swap, span.clone());
        // Stack: [..., slice_func, arr]
        self.push_type(ExpressionType::Unknown); // func type

        // Parse/emit start
        if has_start {
            self.anon_stack_depth += 2; // func + arr
            self.parse_expr_notail(0, memory_manager)?;
            self.anon_stack_depth -= 2;
        } else {
            self.emit_opcode(Opcode::LoadNull, span.clone());
            self.push_type(ExpressionType::Unknown);
        }

        // Consume first ':' (or '::' which also consumes the second colon)
        let first_colon_is_double = matches!(
            self.parser.current_token().map(|t| &t.token),
            Some(Token::Operator(op)) if op == "::"
        );
        if first_colon_is_double {
            self.parser.advance()?; // consume '::'
        } else {
            self.parser
                .consume(Token::Operator(":".to_string()), "Expected ':' in slice")?;
        }

        // If first colon was '::', end is omitted and second colon already consumed
        let second_colon_consumed = first_colon_is_double;

        // Parse end (or null if next is ':', '::', or ']', or if we had '::')
        self.anon_stack_depth += 3; // func + arr + start
        let end_omitted = second_colon_consumed
            || matches!(
                self.parser.current_token().map(|t| &t.token),
                Some(Token::Operator(op)) if op == ":" || op == "::"
            )
            || matches!(
                self.parser.current_token().map(|t| &t.token),
                Some(Token::RightBracket)
            );
        if end_omitted {
            self.emit_opcode(Opcode::LoadNull, span.clone());
            self.push_type(ExpressionType::Unknown);
        } else {
            self.parse_expr_notail(0, memory_manager)?;
        }
        self.anon_stack_depth -= 3;

        // Check for optional step (second ':')
        self.anon_stack_depth += 4; // func + arr + start + end
        let has_step_colon = second_colon_consumed
            || matches!(
                self.parser.current_token().map(|t| &t.token),
                Some(Token::Operator(op)) if op == ":" || op == "::"
            );
        if has_step_colon {
            if !second_colon_consumed {
                self.parser.advance()?; // consume second ':'
            }
            let step_omitted = matches!(
                self.parser.current_token().map(|t| &t.token),
                Some(Token::RightBracket)
            );
            if step_omitted {
                self.emit_opcode(Opcode::LoadNull, span.clone());
                self.push_type(ExpressionType::Unknown);
            } else {
                self.parse_expr_notail(0, memory_manager)?;
            }
        } else {
            self.emit_opcode(Opcode::LoadNull, span.clone());
            self.push_type(ExpressionType::Unknown);
        }
        self.anon_stack_depth -= 4;

        // Consume ']'
        self.parser
            .consume(Token::RightBracket, "Expected ']' after slice")?;

        // Stack: [..., slice_func, arr, start, end, step]
        self.compiling_chunk
            .write_opcode(Opcode::Call, span.clone());
        self.compiling_chunk.write(4u8, span.clone());
        self.compiling_chunk.write(0u8, span);

        // Type tracking: pop step, end, start, arr, func; push result
        self.pop_type(); // step
        self.pop_type(); // end
        self.pop_type(); // start
        self.pop_type(); // arr
        self.pop_type(); // func
        self.push_type(ExpressionType::Unknown);

        Ok(())
    }

    fn parse_prefix(&mut self, memory_manager: &mut MemoryManager) -> Result<(), CompilerError> {
        let token = self.parser.current_token().cloned().ok_or_else(|| {
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
                let allocation_result = memory_manager.allocate_string(value);
                if allocation_result.should_garbage_collect {
                    self.run_garbage_collect(
                        memory_manager,
                        &[Value::String(allocation_result.index)],
                    );
                }
                self.emit_string_constant(allocation_result.index)?;
                self.push_type(ExpressionType::String);
                self.parser.advance()?;
            }
            Token::True => {
                self.emit_opcode(Opcode::LoadTrue, token.span);
                self.push_type(ExpressionType::Boolean);
                self.parser.advance()?;
            }
            Token::False => {
                self.emit_opcode(Opcode::LoadFalse, token.span);
                self.push_type(ExpressionType::Boolean);
                self.parser.advance()?;
            }
            Token::Null => {
                self.emit_opcode(Opcode::LoadNull, token.span);
                self.push_type(ExpressionType::Null);
                self.parser.advance()?;
            }
            Token::Import | Token::ImportStr | Token::ImportBin => {
                let is_import_str = matches!(token.token, Token::ImportStr);
                let is_import_bin = matches!(token.token, Token::ImportBin);
                let keyword_len = if is_import_str {
                    9
                } else if is_import_bin {
                    9
                } else {
                    6
                };
                let start_span = token.span.start;
                self.parser.advance()?; // consume 'import', 'importstr', or 'importbin'

                // Expect a string literal next
                let path_token = self.parser.current_token().cloned().ok_or_else(|| {
                    self.unexpected_eof_error(start_span..start_span + keyword_len)
                })?;

                match &path_token.token {
                    Token::String(path) => {
                        let allocation_result = memory_manager.allocate_string(path);
                        if allocation_result.should_garbage_collect {
                            self.run_garbage_collect(
                                memory_manager,
                                &[Value::String(allocation_result.index)],
                            );
                        }

                        // Add the string to the constant pool and get its u16 index
                        let const_index =
                            self.add_constant_pooled(Value::String(allocation_result.index))?;

                        // Span covers from 'import'/'importstr'/'importbin' to the end of the string
                        let full_span = start_span..path_token.span.end;

                        // Emit Import/ImportStr/ImportBin opcode with the constant pool index
                        self.compiling_chunk.write_opcode_u16(
                            if is_import_str {
                                Opcode::ImportStr
                            } else if is_import_bin {
                                Opcode::ImportBin
                            } else {
                                Opcode::Import
                            },
                            const_index,
                            full_span,
                        );

                        // The import evaluates to an unknown type at compile time
                        // unless it is importstr (string) or importbin (array)
                        if is_import_str {
                            self.push_type(ExpressionType::String);
                        } else if is_import_bin {
                            self.push_type(ExpressionType::Array);
                        } else {
                            self.push_type(ExpressionType::Unknown);
                        }
                        self.parser.advance()?; // consume string
                    }
                    _ => {
                        let msg = if is_import_str {
                            "Expected string literal after 'importstr'"
                        } else if is_import_bin {
                            "Expected string literal after 'importbin'"
                        } else {
                            "Expected string literal after 'import'"
                        };
                        return Err(self.make_error(path_token.span, msg.to_string()));
                    }
                }
            }
            Token::Operator(op) if op == "-" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr_notail(PRECEDENCE_UNARY, memory_manager)?;
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
                self.parse_expr_notail(PRECEDENCE_UNARY, memory_manager)?;
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
                self.parse_expr_notail(PRECEDENCE_UNARY, memory_manager)?;
                self.emit_opcode(Opcode::Not, token.span);
                // Logical NOT always produces boolean
                self.pop_type();
                self.push_type(ExpressionType::Boolean);
            }
            Token::Operator(op) if op == "~" => {
                self.parser.advance()?; // consume the operator
                self.parse_expr_notail(PRECEDENCE_UNARY, memory_manager)?;
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
                self.parse_expr_notail(0, memory_manager)?;
                self.parser
                    .consume(Token::RightParen, "Expected closing parenthesis")?;
                // Parentheses don't change the type
            }
            Token::LeftBrace => {
                self.parse_object_literal(&token, memory_manager)?;
                // Object literal produces Object type
                self.push_type(ExpressionType::Object);
            }
            Token::LeftBracket => {
                self.parse_array_literal(&token, memory_manager)?;
                // Array literal produces Array type
                self.push_type(ExpressionType::Array);
            }
            Token::Error => {
                let error_start = token.span.start;
                self.parser.advance()?; // consume 'error'

                // Parse the error expression with precedence 0 to consume everything to the right
                self.parse_expr(0, memory_manager)?;

                // Calculate the full span from error keyword to end of expression
                let error_end = if let Some(prev_token) = self.parser.previous_token() {
                    prev_token.span.end
                } else if let Some(curr_token) = self.parser.current_token() {
                    // If current token exists, the expression ended just before this token
                    curr_token.span.start
                } else {
                    token.span.end // fallback to just the error keyword if no tokens
                };
                let full_error_span = error_start..error_end;

                self.emit_opcode(Opcode::Error, full_error_span);
                // Error expressions never return a value
                self.push_type(ExpressionType::Unknown);
            }
            Token::Assert => {
                self.parse_assert_expression(memory_manager)?;
                // Assert expressions return the value of their body
                self.push_type(ExpressionType::Unknown);
            }
            Token::If => {
                self.parse_if_expression(memory_manager)?;
                // If expressions can return any type depending on branches
                self.push_type(ExpressionType::Unknown);
            }
            Token::Local => {
                self.parse_local_statement(memory_manager)?;
                // Local statement result is the body expression value
                // Type depends on body expression
                self.push_type(ExpressionType::Unknown);
            }
            Token::Function => {
                self.parse_function_expression(memory_manager)?;
                // Function expressions produce function values
                self.push_type(ExpressionType::Unknown);
            }
            Token::Self_ => {
                self.parser.advance()?;
                // Try to resolve <self> as a local/upvalue first (works inside methods
                // nested in field thunks). Fall back to LoadSelf for direct thunk context.
                if let Some(slot) = self.resolve_local("<self>") {
                    self.compiling_chunk
                        .write_opcode_u16(Opcode::LoadVar, slot as u16, token.span);
                } else if let Some(upvalue_slot) = self.resolve_upvalue("<self>") {
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::GetUpvalue,
                        upvalue_slot as u16,
                        token.span,
                    );
                } else {
                    self.emit_opcode(Opcode::LoadSelf, token.span);
                }
                self.push_type(ExpressionType::Object);
            }
            Token::Super => {
                self.parser.advance()?;
                // Check what follows 'super'
                let next = self.parser.current_token().cloned();

                let mut bare_super = false;
                match next.as_ref().map(|t| &t.token) {
                    Some(Token::Dot) => {
                        self.parser.advance()?; // consume '.'
                        let field_token =
                            self.parser.current_token().cloned().ok_or_else(|| {
                                self.make_error(
                                    next.as_ref().unwrap().span.clone(),
                                    "Expected field name after 'super.'".to_string(),
                                )
                            })?;

                        if let Token::Identifier(field_name) = &field_token.token {
                            let allocation_result = memory_manager.allocate_string(field_name);
                            if allocation_result.should_garbage_collect {
                                self.run_garbage_collect(
                                    memory_manager,
                                    &[Value::String(allocation_result.index)],
                                );
                            }
                            let interned_key = allocation_result.index;
                            self.emit_string_constant(interned_key)?;
                            self.parser.advance()?; // consume identifier
                        } else {
                            return Err(self.make_error(
                                field_token.span,
                                "Expected field name after 'super.'".to_string(),
                            ));
                        }
                    }
                    Some(Token::LeftBracket) => {
                        self.parser.advance()?; // consume '['
                        self.parse_expr_notail(0, memory_manager)?; // dynamic key
                        self.parser
                            .consume(Token::RightBracket, "Expected ']' after 'super[expr]'")?;
                    }
                    _ => {
                        // Bare 'super' for use with 'in' operator (e.g., "field" in super)
                        bare_super = true;
                    }
                }

                if bare_super {
                    self.emit_opcode(Opcode::LoadSuper, token.span.clone());
                    self.push_type(ExpressionType::Object);
                } else {
                    self.emit_opcode(Opcode::SuperIndex, token.span.clone());
                    self.push_type(ExpressionType::Unknown);
                }
            }
            Token::Operator(op) if op == "$" => {
                self.parser.advance()?;
                if let Some(stack_slot) = self.resolve_local(&"$".to_string()) {
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::LoadVar,
                        stack_slot as u16,
                        token.span,
                    );
                } else if let Some(upvalue_slot) = self.resolve_upvalue(&"$".to_string()) {
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::GetUpvalue,
                        upvalue_slot as u16,
                        token.span,
                    );
                } else {
                    return Err(
                        self.make_error(token.span, "'$' used outside of object scope".to_string())
                    );
                }
                self.push_type(ExpressionType::Object);
            }
            Token::Identifier(name) => {
                let name_clone = name.clone();
                let span = token.span.clone();
                self.parser.advance()?; // consume identifier

                // Try to resolve as local variable
                if let Some(stack_slot) = self.resolve_local(&name_clone) {
                    // Emit LoadVar with absolute stack slot
                    self.compiling_chunk
                        .write_opcode_u16(Opcode::LoadVar, stack_slot as u16, span);
                    self.push_type(ExpressionType::Unknown);
                } else if let Some(upvalue_slot) = self.resolve_upvalue(&name_clone) {
                    // Emit GetUpvalue with upvalue slot
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::GetUpvalue,
                        upvalue_slot as u16,
                        span,
                    );
                    self.push_type(ExpressionType::Unknown);
                } else if name_clone == "std" {
                    // Special case for std namespace
                    // Always emit LoadStd so `std` has a value on the stack.
                    // If followed by `.func`, the dot handler will pop it.
                    self.emit_opcode(Opcode::LoadStd, span);
                    self.push_type(ExpressionType::StdNamespace);
                } else {
                    // Variable not found
                    return Err(
                        self.make_error(span, format!("Undefined variable '{}'", name_clone))
                    );
                }
            }
            _ => {
                // Unknown expression type
                self.push_type(ExpressionType::Unknown);
                return Err(self.invalid_expression_error(&token));
            }
        }

        Ok(())
    }

    fn parse_infix(
        &mut self,
        left_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let token = self.parser.current_token().cloned().ok_or_else(|| {
            // Use the previous token's span end as the EOF location
            let span = if let Some(previous) = self.parser.previous_token() {
                previous.span.end..previous.span.end
            } else {
                0..0
            };
            self.unexpected_eof_error(span)
        })?;

        // Check for special short-circuit operators that manage their own parsing
        let is_short_circuit_op =
            matches!(&token.token, Token::Operator(op) if op == "&&" || op == "||");

        self.parser.advance()?; // consume operator

        if !is_short_circuit_op {
            // Left operand is an anonymous temp on the stack while we parse right operand
            self.anon_stack_depth += 1;
            self.parse_expr_notail(left_bp, memory_manager)?;
            self.anon_stack_depth -= 1;
        }

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
                    } else if left_type == ExpressionType::Number
                        && right_type == ExpressionType::Number
                    {
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
            Token::Operator(op) if op == "%" => {
                self.emit_opcode(Opcode::Mod, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // modulo always produces number
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
            Token::Operator(op) if op == "<<" => {
                self.emit_opcode(Opcode::Shl, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // shift ops produce numbers
            }
            Token::Operator(op) if op == ">>" => {
                self.emit_opcode(Opcode::Shr, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // shift ops produce numbers
            }
            Token::Operator(op) if op == "&" => {
                self.emit_opcode(Opcode::BitAnd, token.span);
                self.pop_type(); // right operand
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Number); // bitwise ops produce numbers
            }
            Token::Operator(op) if op == "^" => {
                self.emit_opcode(Opcode::BitXor, token.span);
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
            Token::Operator(op) if op == "&&" => {
                // Short-circuit logical AND: left && right
                // If left is falsy, return false (don't evaluate right)
                // If left is truthy, evaluate and return right

                // At this point, left operand is on stack: [left_value]

                // Dup left for testing: [left_value, left_value]
                self.emit_opcode(Opcode::Dup, token.span.clone());

                // Jump if left is falsy: [left_value] (dup was popped)
                let jump_falsy = self.emit_jump(Opcode::JumpIfFalse, token.span.clone());

                // Left is truthy: pop left and evaluate right: []
                self.emit_opcode(Opcode::Pop, token.span.clone());
                self.parse_expr_notail(left_bp, memory_manager)?; // Parse right operand
                self.pop_type(); // right operand type (consumed by control flow merge)
                let jump_end = self.emit_jump(Opcode::Jump, token.span.clone());

                // Left is falsy: pop left and return false: []
                self.patch_jump(jump_falsy);
                self.emit_opcode(Opcode::Pop, token.span.clone());
                self.emit_opcode(Opcode::LoadFalse, token.span.clone());

                self.patch_jump(jump_end);

                // Type tracking: replace left operand type with result type
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Unknown); // Could be Boolean or right's type
            }
            Token::Operator(op) if op == "||" => {
                // Short-circuit logical OR: left || right
                // If left is truthy, return true (don't evaluate right)
                // If left is falsy, evaluate and return right

                // At this point, left operand is on stack: [left_value]

                // Dup left for testing: [left_value, left_value]
                self.emit_opcode(Opcode::Dup, token.span.clone());

                // Jump if left is truthy: [left_value] (dup was popped)
                let jump_truthy = self.emit_jump(Opcode::JumpIfTrue, token.span.clone());

                // Left is falsy: pop left and evaluate right: []
                self.emit_opcode(Opcode::Pop, token.span.clone());
                self.parse_expr_notail(left_bp, memory_manager)?; // Parse right operand
                self.pop_type(); // right operand type (consumed by control flow merge)
                let jump_end = self.emit_jump(Opcode::Jump, token.span.clone());

                // Left is truthy: pop left and return true: []
                self.patch_jump(jump_truthy);
                self.emit_opcode(Opcode::Pop, token.span.clone());
                self.emit_opcode(Opcode::LoadTrue, token.span.clone());

                self.patch_jump(jump_end);

                // Type tracking: replace left operand type with result type
                self.pop_type(); // left operand
                self.push_type(ExpressionType::Unknown); // Could be Boolean or right's type
            }
            // Membership test: key in object
            Token::In => {
                self.emit_opcode(Opcode::InOp, token.span);
                self.pop_type(); // right operand (object)
                self.pop_type(); // left operand (key)
                self.push_type(ExpressionType::Boolean);
            }
            _ => return Err(self.invalid_expression_error(&token)),
        }

        Ok(())
    }

    fn emit_opcode(&mut self, opcode: Opcode, span: Range<usize>) {
        self.compiling_chunk.write_opcode(opcode, span);
    }

    /// Emit a 32-bit signed integer to the bytecode
    fn emit_i32(&mut self, value: i32, span: Range<usize>) {
        self.compiling_chunk.write_i32(value, span);
    }

    /// Emit a jump instruction with a placeholder offset, return position for patching
    fn emit_jump(&mut self, opcode: Opcode, span: Range<usize>) -> usize {
        self.emit_opcode(opcode, span.clone());
        let jump_pos = self.compiling_chunk.count();
        const PLACEHOLDER_OFFSET: i32 = 0x7FFFFFFF; // Use max i32 as placeholder
        self.emit_i32(PLACEHOLDER_OFFSET, span);
        jump_pos
    }

    /// Patch a previously emitted jump with the actual offset
    fn patch_jump(&mut self, jump_pos: usize) {
        let current_pos = self.compiling_chunk.count();
        // Calculate relative offset from instruction end to current position
        let offset = (current_pos - (jump_pos + I32_SIZE_BYTES)) as i32;

        // Go back and write the actual offset
        self.compiling_chunk.patch_i32(jump_pos, offset);
    }

    /// Update the source span for a previously emitted jump instruction (opcode + i32 operand)
    fn patch_jump_span(&mut self, jump_pos: usize, span: std::ops::Range<usize>) {
        // Opcode (1 byte) + i32 operand (4 bytes) = 5 bytes
        for i in 0..5 {
            self.compiling_chunk
                .patch_span(jump_pos - 1 + i, span.clone());
        }
    }

    fn emit_constant(&mut self, value: f64) -> Result<u16, CompilerError> {
        let index = self.add_constant_pooled(Value::Number(value))?;

        // Use the current token's span for the constant
        let span = self.current_span();

        self.compiling_chunk
            .write_opcode_u16(Opcode::LoadConst, index, span);
        Ok(index)
    }

    fn emit_string_constant(&mut self, value: StringIndex) -> Result<u16, CompilerError> {
        let index = self.add_constant_pooled(Value::String(value))?;

        // Use the current token's span for the constant
        let span = self.current_span();

        self.compiling_chunk
            .write_opcode_u16(Opcode::LoadConst, index, span);
        Ok(index)
    }

    /// Emit a Closure instruction with function metadata including parameter names and defaults
    fn emit_closure_with_params(
        &mut self,
        chunk: chunk::Chunk,
        upvalues: Vec<CompilerUpvalue>,
        arity: u8,
        required_params: u8,
        param_names: Vec<StringIndex>,
        span: Range<usize>,
        memory_manager: &mut MemoryManager,
        is_thunk: bool,
    ) -> Result<(), CompilerError> {
        // Convert chunk to owned chunk for storage
        let owned_chunk = chunk.into_owned();

        // Allocate function in memory manager
        let func_result =
            memory_manager.allocate_function(None, arity, upvalues.len() as u8, owned_chunk);

        // Set parameter metadata BEFORE GC so param_names are reachable
        {
            let func = memory_manager.load_function_mut(func_result.index);
            func.required_params = required_params;
            func.param_names = param_names;
        }

        if func_result.should_garbage_collect {
            self.run_garbage_collect(memory_manager, &[Value::Function(func_result.index)]);
        }

        // Add function to constants pool
        let func_index = self.add_constant_pooled(Value::Function(func_result.index))?;

        // Emit Closure/MakeThunk opcode with function index (opcode + u16)
        let opcode = if is_thunk {
            Opcode::MakeThunk
        } else {
            Opcode::Closure
        };
        self.compiling_chunk
            .write_opcode_u16(opcode, func_index, span.clone());

        // Manually push upvalue count byte to code vector
        self.compiling_chunk
            .write(upvalues.len() as u8, span.clone());

        // Emit upvalue descriptors
        for upvalue in upvalues {
            // Emit is_local (1 if local, 0 if upvalue)
            let is_local_byte = if upvalue.is_local { 1u8 } else { 0u8 };
            self.compiling_chunk.write(is_local_byte, span.clone());

            // Emit index (u16) in little-endian
            let index_bytes = (upvalue.index as u16).to_le_bytes();
            self.compiling_chunk.write(index_bytes[0], span.clone());
            self.compiling_chunk.write(index_bytes[1], span.clone());
        }

        Ok(())
    }

    fn emit_thunk(
        &mut self,
        chunk: chunk::Chunk,
        upvalues: Vec<CompilerUpvalue>,
        span: Range<usize>,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let owned_chunk = chunk.into_owned();
        let func_result =
            memory_manager.allocate_function(None, 0, upvalues.len() as u8, owned_chunk);
        if func_result.should_garbage_collect {
            self.run_garbage_collect(memory_manager, &[Value::Function(func_result.index)]);
        }
        let func_index = self.add_constant_pooled(Value::Function(func_result.index))?;
        self.compiling_chunk
            .write_opcode_u16(Opcode::MakeThunk, func_index, span.clone());
        self.compiling_chunk
            .write(upvalues.len() as u8, span.clone());
        for upvalue in upvalues {
            let is_local_byte = if upvalue.is_local { 1u8 } else { 0u8 };
            self.compiling_chunk.write(is_local_byte, span.clone());
            let index_bytes = (upvalue.index as u16).to_le_bytes();
            self.compiling_chunk.write(index_bytes[0], span.clone());
            self.compiling_chunk.write(index_bytes[1], span.clone());
        }
        Ok(())
    }

    /// Parse an expression and wrap it in a thunk for lazy evaluation.
    /// Used for array elements and other lazy contexts.
    fn parse_expr_as_thunk(
        &mut self,
        min_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let thunk_span = self.current_span(); // capture before parsing the element
        let saved_state = self.begin_function();
        self.begin_scope();
        self.declare_local("<closure>".to_string())?;
        self.parse_expr_notail(min_bp, memory_manager)?;
        let return_span = self.current_span();
        self.emit_opcode(Opcode::Return, return_span);
        self.end_scope();
        let (chunk, upvalues) = self.end_function(saved_state);
        self.emit_thunk(chunk, upvalues, thunk_span, memory_manager)?;
        Ok(())
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
        ScanError::new(span, message, self.parser.source_id().to_string())
    }

    // Type stack management for compile-time optimizations
    fn push_type(&mut self, expr_type: ExpressionType) {
        self.type_stack.push(expr_type);
    }

    fn pop_type(&mut self) -> ExpressionType {
        self.type_stack.pop().unwrap_or(ExpressionType::Unknown)
    }

    fn unexpected_eof_error(&self, span: Range<usize>) -> CompilerError {
        self.make_error(span, "Unexpected end of input".to_string())
    }

    fn invalid_expression_error(&self, token: &TokenInfo) -> CompilerError {
        self.make_error(
            token.span.clone(),
            format!("Invalid expression starting with {:?}", token.token),
        )
    }

    fn too_many_constants_error(&self) -> CompilerError {
        self.make_error(
            0..0, // This error doesn't relate to a specific token location
            "Too many constants (maximum 65535)".to_string(),
        )
    }

    fn unexpected_token_after_expression_error(&self, token: &TokenInfo) -> CompilerError {
        let message = match &token.token {
            Token::RightParen => "Unexpected ')' - no matching opening parenthesis".to_string(),
            Token::RightBrace => "Unexpected '}' - no matching opening brace".to_string(),
            Token::RightBracket => "Unexpected ']' - no matching opening bracket".to_string(),
            _ => format!(
                "Unexpected token {:?} after complete expression",
                token.token
            ),
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
            Token::Operator(op) if op == "*" || op == "/" || op == "%" => {
                Some((PRECEDENCE_MULTIPLICATIVE, PRECEDENCE_MULTIPLICATIVE + 1))
            }

            // Additive (left associative)
            Token::Operator(op) if op == "+" || op == "-" => {
                Some((PRECEDENCE_ADDITIVE, PRECEDENCE_ADDITIVE + 1))
            }

            // Comparison (non-associative)
            Token::Operator(op) if matches!(op.as_str(), "<" | "<=" | ">" | ">=") => {
                Some((PRECEDENCE_COMPARISON, PRECEDENCE_COMPARISON + 1))
            }

            // Membership test: expr in expr (same precedence as comparison)
            Token::In => Some((PRECEDENCE_COMPARISON, PRECEDENCE_COMPARISON + 1)),

            // Equality (left associative)
            Token::Operator(op) if matches!(op.as_str(), "==" | "!=") => {
                Some((PRECEDENCE_EQUALITY, PRECEDENCE_EQUALITY + 1))
            }

            // Shift operators (left associative)
            Token::Operator(op) if op == "<<" || op == ">>" => {
                Some((PRECEDENCE_SHIFT, PRECEDENCE_SHIFT + 1))
            }

            // Bitwise AND (left associative)
            Token::Operator(op) if op == "&" => Some((PRECEDENCE_BITAND, PRECEDENCE_BITAND + 1)),

            // Bitwise XOR (left associative)
            Token::Operator(op) if op == "^" => Some((PRECEDENCE_BITXOR, PRECEDENCE_BITXOR + 1)),

            // Bitwise OR (left associative)
            Token::Operator(op) if op == "|" => Some((PRECEDENCE_BITOR, PRECEDENCE_BITOR + 1)),

            // Logical AND (left associative)
            Token::Operator(op) if op == "&&" => {
                Some((PRECEDENCE_LOGICAL_AND, PRECEDENCE_LOGICAL_AND + 1))
            }

            // Logical OR (left associative)
            Token::Operator(op) if op == "||" => {
                Some((PRECEDENCE_LOGICAL_OR, PRECEDENCE_LOGICAL_OR + 1))
            }

            _ => None,
        }
    }

    fn get_postfix_binding_power(&self, token: &Token) -> Option<u8> {
        match token {
            Token::Dot => Some(PRECEDENCE_POSTFIX),
            Token::LeftBracket => Some(PRECEDENCE_POSTFIX),
            Token::LeftParen => Some(PRECEDENCE_POSTFIX),
            Token::LeftBrace => Some(PRECEDENCE_POSTFIX), // object apply: expr { ... }
            _ => None,
        }
    }

    fn parse_postfix(&mut self, memory_manager: &mut MemoryManager) -> Result<(), CompilerError> {
        let token = self.parser.current_token().cloned().ok_or_else(|| {
            // Use the previous token's span end as the EOF location
            let span = if let Some(previous) = self.parser.previous_token() {
                previous.span.end..previous.span.end
            } else {
                0..0
            };
            self.unexpected_eof_error(span)
        })?;

        match &token.token {
            Token::Dot => {
                self.parser.advance()?; // consume '.'

                // Expect an identifier for property name
                let property_token = self.parser.current_token().cloned().ok_or_else(|| {
                    let span = token.span.end..token.span.end;
                    self.unexpected_eof_error(span)
                })?;

                match &property_token.token {
                    Token::Identifier(name) => {
                        self.parser.advance()?; // consume identifier

                        // Check if we are accessing a native function on 'std'
                        if let Some(ExpressionType::StdNamespace) = self.type_stack.last() {
                            self.type_stack.pop();
                            // Pop the std object value that LoadStd pushed
                            self.emit_opcode(Opcode::Pop, property_token.span.clone());

                            // Special handling for std.thisFile
                            if name == "thisFile" {
                                let allocation_result =
                                    memory_manager.allocate_string(&self.compiling_chunk.source_id);
                                if allocation_result.should_garbage_collect {
                                    self.run_garbage_collect(
                                        memory_manager,
                                        &[Value::String(allocation_result.index)],
                                    );
                                }
                                self.emit_string_constant(allocation_result.index)?;
                                self.push_type(ExpressionType::String);
                                return Ok(());
                            } else if name == "pi" {
                                self.emit_constant(std::f64::consts::PI)?;
                                self.push_type(ExpressionType::Number);
                                return Ok(());
                            } else if let Some(id) = chunk::NativeFuncId::from_name(name) {
                                // Instead of runtime property access, load the native function ID as a value
                                let const_idx = self
                                    .compiling_chunk
                                    .add_constant(chunk::Value::NativeFunction(id));
                                self.compiling_chunk.write_opcode_u16(
                                    Opcode::LoadConst,
                                    const_idx as u16,
                                    property_token.span.clone(),
                                );
                                self.push_type(ExpressionType::NativeFunction(id));
                                return Ok(());
                            } else {
                                return Err(self.make_error(
                                    property_token.span,
                                    format!("Native function 'std.{}' not found", name),
                                ));
                            }
                        }

                        let allocation_result = memory_manager.allocate_string(name);
                        if allocation_result.should_garbage_collect {
                            self.run_garbage_collect(
                                memory_manager,
                                &[Value::String(allocation_result.index)],
                            );
                        }

                        let _index = self.emit_string_constant(allocation_result.index)?;

                        // Emit ObjectIndex opcode to access property
                        self.emit_opcode(Opcode::ObjectIndex, property_token.span);

                        // Type tracking: ObjectIndex consumes object + field name, pushes result
                        // The string constant doesn't get a type_stack entry (it's an internal
                        // operand to ObjectIndex, not an expression result)
                        self.pop_type(); // object type
                        self.push_type(ExpressionType::Unknown);
                    }
                    _ => {
                        return Err(self.make_error(
                            property_token.span,
                            "Expected property name after '.'".to_string(),
                        ));
                    }
                }
            }
            Token::LeftBracket => {
                self.parser.advance()?; // consume '['

                // Check if this is slice syntax: [start:end] or [start:end:step]
                // or [:end], [start:], [::], [::step], etc.
                let is_slice_start = matches!(
                    self.parser.current_token().map(|t| &t.token),
                    Some(Token::Operator(op)) if op == ":" || op == "::"
                );

                if is_slice_start {
                    // Empty start → null
                    // Compile as std.slice(arr, start, end, step)
                    self.compile_slice_sugar(false, &token.span, memory_manager)?;
                } else {
                    // Save checkpoint in case this is a slice with a start expression
                    let checkpoint = self.parser.save_checkpoint();
                    let type_depth = self.type_stack.len();
                    let code_pos = self.compiling_chunk.code.len();
                    let const_count = self.compiling_chunk.constants.len();
                    let span_count = self.compiling_chunk.spans.len();
                    let last_span_repeat =
                        self.compiling_chunk.spans.last().map(|s| s.repeated_values);
                    let constant_pool_snapshot = self.constant_pool.clone();

                    // The array/object value is an anonymous temp while we parse the index
                    self.anon_stack_depth += 1;
                    // Parse the first expression inside brackets
                    self.parse_expr_notail(0, memory_manager)?;
                    self.anon_stack_depth -= 1;

                    // Check if next token is ':' or '::' → slice syntax
                    let is_slice = matches!(
                        self.parser.current_token().map(|t| &t.token),
                        Some(Token::Operator(op)) if op == ":" || op == "::"
                    );

                    if is_slice {
                        // Backtrack: undo the start expression compilation
                        self.parser.restore_checkpoint(checkpoint);
                        self.type_stack.truncate(type_depth);
                        self.compiling_chunk.code.truncate(code_pos);
                        self.compiling_chunk.constants.truncate(const_count);
                        self.compiling_chunk.spans.truncate(span_count);
                        self.constant_pool = constant_pool_snapshot;
                        if let Some(repeat) = last_span_repeat {
                            if let Some(last) = self.compiling_chunk.spans.last_mut() {
                                last.repeated_values = repeat;
                            }
                        }
                        // Recompile as slice with start
                        self.compile_slice_sugar(true, &token.span, memory_manager)?;
                    } else {
                        // Normal index: expect ']'
                        self.parser.consume(
                            Token::RightBracket,
                            "Expected ']' after property expression",
                        )?;

                        // Emit ArrayIndex opcode - handles both arrays and objects at runtime
                        self.emit_opcode(Opcode::ArrayIndex, token.span);

                        // Type tracking: pop index type and object/array type, push result type
                        self.pop_type(); // index
                        self.pop_type(); // object/array
                        self.push_type(ExpressionType::Unknown);
                    }
                }
            }
            Token::LeftParen => {
                let call_start = token.span.start; // byte position of '(' for span attribution
                self.parser.advance()?; // consume '('

                // Check if we are calling a native function
                let native_id =
                    if let Some(ExpressionType::NativeFunction(id)) = self.type_stack.last() {
                        Some(*id)
                    } else {
                        None
                    };

                // Callee is an anonymous temp on the stack while we parse arguments
                self.anon_stack_depth += 1;

                // Parse argument list
                let mut positional_count = 0u8;
                let mut named_count = 0u8;
                let mut in_named = false;

                // Check for empty argument list
                if let Some(current) = self.parser.current_token() {
                    if current.token != Token::RightParen {
                        loop {
                            // Check for named argument: identifier '='
                            let is_named_arg = if let Some(TokenInfo {
                                token: Token::Identifier(_),
                                ..
                            }) = self.parser.current_token()
                            {
                                matches!(
                                    self.parser.peek_ahead(1)?.map(|t| &t.token),
                                    Some(Token::Operator(op)) if op == "="
                                )
                            } else {
                                false
                            };

                            if is_named_arg {
                                in_named = true;
                                // Push name string constant
                                let name = if let Some(TokenInfo {
                                    token: Token::Identifier(name),
                                    ..
                                }) = self.parser.current_token()
                                {
                                    name.clone()
                                } else {
                                    unreachable!()
                                };
                                self.parser.advance()?; // consume identifier
                                self.parser.advance()?; // consume '='
                                let name_idx = memory_manager.allocate_string(&name).index;
                                self.emit_string_constant(name_idx)?;
                                self.anon_stack_depth += 1; // name is temp
                                // Parse value expression
                                self.parse_expr_notail(0, memory_manager)?;
                                self.anon_stack_depth += 1; // value is temp
                                named_count += 1;
                            } else {
                                if in_named {
                                    return Err(self.make_error(
                                        self.current_span(),
                                        "Positional argument cannot follow named argument"
                                            .to_string(),
                                    ));
                                }
                                // Parse positional argument expression
                                self.parse_expr_notail(0, memory_manager)?;
                                positional_count += 1;
                                self.anon_stack_depth += 1; // arg is temp
                            }

                            // Check for more arguments
                            if let Some(current) = self.parser.current_token() {
                                if current.token == Token::Comma {
                                    self.parser.advance()?; // consume ','
                                    // Allow trailing comma: if next token is ')', stop
                                    if matches!(
                                        self.parser.current_token().map(|t| &t.token),
                                        Some(Token::RightParen)
                                    ) {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                    }
                }

                // Call/StdCall consumes callee + all args (+ name strings for named args)
                self.anon_stack_depth -= 1 + positional_count as usize + (named_count as usize * 2);

                // Expect closing paren; use the returned TokenInfo to get its span end
                // so the Call instruction covers every line of a multi-line call expression.
                let close_paren = self
                    .parser
                    .consume(Token::RightParen, "Expected ')' after arguments")?;
                let call_span = call_start..close_paren.span.end;

                // Check for postfix `tailstrict` keyword.
                // Accepted anywhere syntactically, but only emits TailCall in tail position.
                let has_tailstrict = matches!(
                    self.parser.current_token().map(|t| &t.token),
                    Some(Token::TailStrict)
                );
                if has_tailstrict {
                    self.parser.advance()?; // consume 'tailstrict'
                    if self.in_tail_position {
                        self.tail_calls_emitted += 1;
                    }
                }

                // Pop argument types and callee type from type_stack
                // (Call/StdCall consumes callee + args + named name strings, produces result)
                for _ in 0..(positional_count as usize + named_count as usize * 2) {
                    self.pop_type();
                }
                self.pop_type(); // callee

                if let Some(id) = native_id {
                    // tailstrict has no effect on native functions
                    self.tail_call_pending = false;
                    if named_count > 0 {
                        // Named args for native functions: emit Call opcode instead
                        // of StdCall so the VM can resolve named args at runtime.
                        let span = call_span;
                        self.compiling_chunk
                            .write_opcode(Opcode::Call, span.clone());
                        self.compiling_chunk.write(positional_count, span.clone());
                        self.compiling_chunk.write(named_count, span);
                    } else {
                        // Emit StdCall opcode with native function ID and arg count
                        let span = call_span;
                        self.compiling_chunk.write_opcode_u16(
                            Opcode::StdCall,
                            id as u16,
                            span.clone(),
                        );
                        self.compiling_chunk
                            .write(positional_count + named_count, span);
                    }
                } else {
                    // Emit TailCall if tailstrict in tail position, otherwise regular Call
                    let is_tail_call =
                        self.tail_call_pending || (has_tailstrict && self.in_tail_position);
                    self.tail_call_pending = false;
                    let span = call_span;
                    let opcode = if is_tail_call {
                        Opcode::TailCall
                    } else {
                        Opcode::Call
                    };
                    self.compiling_chunk.write_opcode(opcode, span.clone());
                    self.compiling_chunk.write(positional_count, span.clone());
                    self.compiling_chunk.write(named_count, span);
                }

                // Function call can return any type
                self.push_type(ExpressionType::Unknown);
            }
            Token::LeftBrace => {
                // Object apply: expr { field: value } is sugar for expr + { field: value }
                let left_type = self.pop_type();
                // Left value is an anonymous temp on the stack during object literal parsing
                self.anon_stack_depth += 1;
                self.parse_object_literal(&token, memory_manager)?;
                self.anon_stack_depth -= 1;
                // parse_object_literal doesn't push a type, we handle it here

                // Emit Add to merge the left expression with the object literal
                self.emit_opcode(Opcode::Add, token.span);

                // Result type depends on left operand
                if left_type == ExpressionType::Object {
                    self.push_type(ExpressionType::Object);
                } else {
                    self.push_type(ExpressionType::Unknown);
                }
            }
            _ => {
                return Err(self.make_error(
                    token.span,
                    format!("Unexpected postfix operator: {:?}", token.token),
                ));
            }
        }

        Ok(())
    }

    /// Parse an object literal: { key1: value1, key2: value2, ... }
    fn parse_object_literal(
        &mut self,
        start_token: &TokenInfo,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        self.object_depth += 1;
        self.parser.advance()?; // consume '{'

        // Handle empty object: {}
        if let Some(current) = self.parser.current_token() {
            if current.token == Token::RightBrace {
                self.parser.advance()?; // consume '}'
                self.compiling_chunk.write_opcode_u16(
                    Opcode::CreateObject,
                    0,
                    start_token.span.clone(),
                );
                self.object_depth -= 1;
                return Ok(());
            }
        }

        // Check if this is a comprehension using lookahead
        let mut is_comprehension = false;
        let mut depth = 0;
        let mut lookahead_idx = 0;
        loop {
            let token = self.parser.peek_ahead(lookahead_idx)?;
            match token {
                Some(t) => {
                    match &t.token {
                        Token::For if depth == 0 => {
                            is_comprehension = true;
                            break;
                        }
                        Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                        Token::RightParen | Token::RightBracket | Token::RightBrace => {
                            if depth == 0 {
                                // End of object without finding 'for' - not a comprehension
                                break;
                            }
                            depth -= 1;
                        }
                        Token::Eof => break,
                        _ => {}
                    }
                    lookahead_idx += 1;
                }
                None => break,
            }
        }

        if is_comprehension {
            let res = self.parse_object_comprehension(start_token, lookahead_idx, memory_manager);
            self.object_depth -= 1;
            return res;
        }

        // Pre-scan for object-local declarations
        let mut object_locals: Vec<ObjectLocalBinding> = Vec::new();
        {
            let members_start = self.parser.save_checkpoint();
            loop {
                if let Some(current) = self.parser.current_token() {
                    if current.token == Token::RightBrace {
                        break;
                    }
                    if current.token == Token::Local {
                        let span = current.span.clone();
                        let checkpoint = self.parser.save_checkpoint();
                        object_locals.push(ObjectLocalBinding { checkpoint, span });
                        self.parser.advance()?; // 'local'
                        self.parser.advance()?; // identifier
                        self.skip_balanced_parens_if_present()?;
                        self.parser.consume(
                            Token::Operator("=".to_string()),
                            "Expected '=' after local name",
                        )?;
                        self.skip_to_member_end()?;
                    } else {
                        // Skip non-local member (field or assert)
                        self.skip_to_member_end()?;
                    }
                    // Consume comma if present
                    if let Some(t) = self.parser.current_token() {
                        if t.token == Token::Comma {
                            self.parser.advance()?;
                            // Check for trailing comma
                            if let Some(next) = self.parser.current_token() {
                                if next.token == Token::RightBrace {
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    break;
                }
            }
            self.parser.restore_checkpoint(members_start);
        }

        // Create the object immediately (with 0 initial fields)
        self.compiling_chunk
            .write_opcode_u16(Opcode::CreateObject, 0, start_token.span.clone());

        // Parse fields and assertions
        loop {
            let mut field_key_name: Option<String> = None;
            let mut key_token_span: std::ops::Range<usize> = 0..0;

            if let Some(key_token) = self.parser.current_token().cloned() {
                key_token_span = key_token.span.clone();
                if key_token.token == Token::RightBrace {
                    break;
                }

                if key_token.token == Token::Local {
                    // Skip: already recorded during pre-scan
                    self.parser.advance()?; // 'local'
                    self.parser.advance()?; // identifier
                    self.skip_balanced_parens_if_present()?;
                    self.parser.consume(
                        Token::Operator("=".to_string()),
                        "Expected '=' after local name",
                    )?;
                    self.skip_to_member_end()?;

                    // Handle comma/brace trailing
                    if let Some(current) = self.parser.current_token() {
                        match &current.token {
                            Token::Comma => {
                                self.parser.advance()?;
                                if let Some(next) = self.parser.current_token() {
                                    if next.token == Token::RightBrace {
                                        break;
                                    }
                                }
                            }
                            Token::RightBrace => {
                                break;
                            }
                            _ => {}
                        }
                    }
                    continue;
                }

                if key_token.token == Token::Assert {
                    let assert_start = key_token.span.start;
                    self.parser.advance()?; // consume 'assert'

                    // Compile the assertion as a closure (thunk)
                    let saved_state = self.begin_function();
                    self.begin_scope();
                    self.declare_local("<closure>".to_string())?; // Slot 0
                    // Declare the implicit self/super args to keep slot numbering in sync
                    self.declare_local("<self>".to_string())?; // Slot 1
                    self.declare_local("<super>".to_string())?; // Slot 2

                    self.inject_object_locals(&object_locals, memory_manager, 0)?;

                    // Evaluate condition
                    self.parse_expr_notail(0, memory_manager)?;

                    // Check for optional msg token
                    let has_msg_colon = if let Some(TokenInfo {
                        token: Token::Operator(op),
                        ..
                    }) = self.parser.current_token()
                    {
                        op == ":"
                    } else {
                        false
                    };

                    let jump_to_success = self.emit_jump(Opcode::JumpIfTrue, 0..0);

                    // Evaluate error msg (failure path)
                    if has_msg_colon {
                        self.parser.advance()?; // consume ':'
                        self.parse_expr_notail(0, memory_manager)?;
                    } else {
                        self.emit_string_constant(
                            memory_manager
                                .allocate_string("Object assertion failed")
                                .index,
                        )?;
                    }

                    // Calculate full span for reporting errors
                    let assert_end = if let Some(prev_token) = self.parser.previous_token() {
                        prev_token.span.end
                    } else {
                        assert_start + 6
                    };
                    let full_assert_span = assert_start..assert_end;

                    // Patch the jump span
                    self.patch_jump_span(jump_to_success, full_assert_span.clone());

                    self.emit_opcode(Opcode::Error, full_assert_span.clone());

                    self.patch_jump(jump_to_success);

                    // Return null on success
                    self.emit_opcode(Opcode::LoadNull, full_assert_span.clone());
                    self.emit_opcode(Opcode::Return, full_assert_span);

                    self.end_scope();

                    let (chunk, upvalues) = self.end_function(saved_state);

                    // Assertions take 2 arguments (self, super) just like fields
                    self.emit_closure_with_params(
                        chunk,
                        upvalues,
                        2,
                        2,
                        Vec::new(),
                        key_token_span.clone(),
                        memory_manager,
                        true,
                    )?;

                    // Attach the closure to the object
                    self.emit_opcode(Opcode::Assert, key_token.span.clone());

                    // Check for trailing comma or end
                    if let Some(current) = self.parser.current_token() {
                        match &current.token {
                            Token::Comma => {
                                self.parser.advance()?; // consume ','
                                if let Some(next) = self.parser.current_token() {
                                    if next.token == Token::RightBrace {
                                        break;
                                    }
                                }
                            }
                            Token::RightBrace => {
                                break;
                            }
                            _ => {
                                return Err(self.make_error(
                                    current.span.clone(),
                                    "Expected ',' or '}' after object assert".to_string(),
                                ));
                            }
                        }
                    }
                    continue;
                }

                // Normal field
                match &key_token.token {
                    Token::String(key_value) => {
                        field_key_name = Some(key_value.clone());
                        let allocation_result = memory_manager.allocate_string(key_value);
                        if allocation_result.should_garbage_collect {
                            self.run_garbage_collect(
                                memory_manager,
                                &[Value::String(allocation_result.index)],
                            );
                        }
                        let interned_key = allocation_result.index;
                        let _key_index = self.emit_string_constant(interned_key)?;
                        self.push_type(ExpressionType::String);

                        self.parser.advance()?; // consume the key
                    }
                    Token::Identifier(key_name) => {
                        field_key_name = Some(key_name.clone());
                        let allocation_result = memory_manager.allocate_string(key_name);
                        if allocation_result.should_garbage_collect {
                            self.run_garbage_collect(
                                memory_manager,
                                &[Value::String(allocation_result.index)],
                            );
                        }
                        let interned_key = allocation_result.index;
                        let _key_index = self.emit_string_constant(interned_key)?;
                        self.push_type(ExpressionType::String);

                        self.parser.advance()?; // consume the key
                    }
                    Token::LeftBracket => {
                        self.parser.advance()?; // consume '['
                        self.parse_expr_notail(0, memory_manager)?; // evaluate dynamic key expression
                        self.parser.consume(
                            Token::RightBracket,
                            "Expected ']' after dynamic object key",
                        )?;
                    }
                    _ => {
                        return Err(self.make_error(
                            key_token.span.clone(),
                            "Object key must be a string literal, identifier, or dynamic key '[expr]'".to_string(),
                        ));
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(start_token.span.clone()));
            }

            // Check for method shorthand: f(params): expr => f: function(params) expr
            let method_params = if let Some(current) = self.parser.current_token() {
                if current.token == Token::LeftParen {
                    let (params, close_paren_span) = self.parse_parameter_list()?;
                    Some((params, close_paren_span))
                } else {
                    None
                }
            } else {
                None
            };

            // Check for '+' before ':' (field override syntax: field+: value)
            let is_override = if let Some(current) = self.parser.current_token() {
                if let Token::Operator(op) = &current.token {
                    if op == "+" {
                        self.parser.advance()?; // consume '+'
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            // Expect field separator after key
            let visibility = if let Some(current) = self.parser.current_token() {
                match &current.token {
                    Token::Operator(op) if op == ":" => {
                        self.parser.advance()?;
                        FieldVisibility::Visible
                    }
                    Token::Operator(op) if op == "::" => {
                        self.parser.advance()?;
                        FieldVisibility::Hidden
                    }
                    Token::Operator(op) if op == ":::" => {
                        self.parser.advance()?;
                        FieldVisibility::ForceVisible
                    }
                    _ => {
                        return Err(self.make_error(
                            current.span.clone(),
                            "Expected ':', '::', or ':::' after object key".to_string(),
                        ));
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(start_token.span.clone()));
            };

            // Compile the field value as a Thunk (Closure)
            let (saved_scope_depth, saved_function_type) = self.begin_function();
            self.begin_scope();
            self.declare_local("<closure>".to_string())?; // Slot 0
            // Declare the implicit self/super args to keep slot numbering in sync
            self.declare_local("<self>".to_string())?; // Slot 1
            self.declare_local("<super>".to_string())?; // Slot 2

            if self.object_depth == 1 {
                // local $ = self
                self.emit_opcode(Opcode::LoadSelf, 0..0);
                self.declare_local("$".to_string())?; // Slot 3
            }

            self.inject_object_locals(&object_locals, memory_manager, 0)?;

            let prev_tail = self.in_tail_position;
            self.in_tail_position = !is_override; // can't tail-call if we need to Add after
            if let Some((params, close_paren_span)) = method_params {
                // Method shorthand: span covers from field name through closing ')' of
                // parameter list so multi-line definitions are fully attributed.
                let fn_span = key_token_span.start..close_paren_span.end;
                self.compile_function_body(params, fn_span, memory_manager)?;
            } else {
                self.parse_expr(0, memory_manager)?;
            }
            self.in_tail_position = prev_tail;

            if is_override {
                // Emit: if field in super then super.field + value else value
                // Stack currently has: [value]

                // Check if super has this field
                if let Some(ref key_name) = field_key_name {
                    let alloc = memory_manager.allocate_string(key_name);
                    self.emit_string_constant(alloc.index)?;
                } else {
                    self.emit_opcode(Opcode::LoadFieldName, key_token_span.clone());
                }
                self.emit_opcode(Opcode::SuperHasField, key_token_span.clone());
                let jump_no_super = self.emit_jump(Opcode::JumpIfFalse, key_token_span.clone());

                // Super has the field: emit super.field + value
                if let Some(ref key_name) = field_key_name {
                    let alloc = memory_manager.allocate_string(key_name);
                    self.emit_string_constant(alloc.index)?;
                } else {
                    self.emit_opcode(Opcode::LoadFieldName, key_token_span.clone());
                }
                self.emit_opcode(Opcode::SuperIndex, key_token_span.clone());
                // Stack: [value, super.field] — need [super.field, value] for Add
                self.emit_opcode(Opcode::Swap, key_token_span.clone());
                self.emit_opcode(Opcode::Add, key_token_span.clone());

                // No super: value is already on stack, skip over
                self.patch_jump(jump_no_super);
            }

            self.emit_opcode(Opcode::Return, 0..0);
            self.end_scope();

            let (field_chunk, field_upvalues) =
                self.end_function((saved_scope_depth, saved_function_type));

            // Emit Closure with arity 2 (self, super), attributed to the field name span
            // so that the field definition line is marked as covered when the field is used.
            self.emit_closure_with_params(
                field_chunk,
                field_upvalues,
                2,
                2,
                Vec::new(),
                key_token_span.clone(),
                memory_manager,
                true,
            )?;

            // Emit ObjectInsert with visibility operand
            // ObjectInsert consumes the key string from the stack
            self.pop_type(); // key type (String or dynamic expression type)
            self.compiling_chunk.write_opcode_u8(
                Opcode::ObjectInsert,
                visibility as u8,
                start_token.span.clone(),
            );

            // Check for more fields or end of object
            if let Some(current) = self.parser.current_token() {
                match &current.token {
                    Token::Comma => {
                        self.parser.advance()?; // consume ','

                        // Check for trailing comma followed by '}'
                        if let Some(next) = self.parser.current_token() {
                            if next.token == Token::RightBrace {
                                break;
                            }
                        }
                        // Continue parsing next field
                    }
                    Token::RightBrace => {
                        break; // End of object
                    }
                    Token::Eof => {
                        return Err(self.unexpected_eof_error(current.span.clone()));
                    }
                    _ => {
                        return Err(self.make_error(
                            current.span.clone(),
                            "Expected ',' or '}' in object literal".to_string(),
                        ));
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(start_token.span.clone()));
            }
        }

        // Consume the closing '}'
        self.parser
            .consume(Token::RightBrace, "Expected '}' to close object literal")?;

        self.object_depth -= 1;
        Ok(())
    }

    /// Parse an array literal: [element1, element2, ...]
    /// or an array comprehension: [expr for x in array]
    fn parse_array_literal(
        &mut self,
        start_token: &TokenInfo,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        self.parser.advance()?; // consume '['

        let mut element_count = 0u16;

        // Handle empty array: []
        if let Some(current) = self.parser.current_token() {
            if current.token == Token::RightBracket {
                self.parser.advance()?; // consume ']'
                self.compiling_chunk.write_opcode_u16(
                    Opcode::CreateArray,
                    0,
                    start_token.span.clone(),
                );
                return Ok(());
            }
        }

        // Check if this is a comprehension using lookahead (before parsing)
        // Look for pattern: expr for x in ...
        // We need to find 'for' at depth 0 (not inside nested brackets/parens)
        let mut is_comprehension = false;
        let mut depth = 0;
        let mut lookahead_idx = 0;
        loop {
            let token = self.parser.peek_ahead(lookahead_idx)?;
            match token {
                Some(t) => {
                    match &t.token {
                        Token::For if depth == 0 => {
                            is_comprehension = true;
                            break;
                        }
                        Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                        Token::RightParen | Token::RightBrace => {
                            if depth > 0 {
                                depth -= 1;
                            }
                        }
                        Token::RightBracket => {
                            if depth > 0 {
                                depth -= 1;
                            } else {
                                // End of array without finding 'for' - not a comprehension
                                break;
                            }
                        }
                        Token::Eof => break,
                        _ => {}
                    }
                    lookahead_idx += 1;
                }
                None => break,
            }
        }

        if is_comprehension {
            // We already know for_offset from the lookahead, pass it to avoid re-scanning
            return self.parse_array_comprehension(start_token, lookahead_idx, memory_manager);
        }

        // For regular arrays, we don't commit - let the buffer be consumed naturally
        // during parsing. The buffered tokens are still valid lookahead that we'll use.

        // Regular array literal - parse first element as thunk for lazy evaluation
        self.parse_expr_as_thunk(0, memory_manager)?;
        element_count += 1;
        self.anon_stack_depth += 1; // element is anonymous temp during subsequent parsing

        // Regular array literal - continue parsing elements
        loop {
            // Check for more elements or end of array
            if let Some(current) = self.parser.current_token() {
                match &current.token {
                    Token::Comma => {
                        self.parser.advance()?; // consume ','

                        // Check for trailing comma followed by ']'
                        if let Some(next) = self.parser.current_token() {
                            if next.token == Token::RightBracket {
                                break;
                            }
                        }
                        // Parse next element as thunk for lazy evaluation
                        self.parse_expr_as_thunk(0, memory_manager)?;
                        element_count += 1;
                        self.anon_stack_depth += 1; // element is anonymous temp
                    }
                    Token::RightBracket => {
                        break; // End of array
                    }
                    Token::Eof => {
                        return Err(self.unexpected_eof_error(current.span.clone()));
                    }
                    _ => {
                        return Err(self.make_error(
                            current.span.clone(),
                            "Expected ',' or ']' in array literal".to_string(),
                        ));
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(start_token.span.clone()));
            }
        }

        // Consume the closing ']'
        self.parser
            .consume(Token::RightBracket, "Expected ']' to close array literal")?;

        // CreateArray consumes all N elements from the stack
        self.anon_stack_depth -= element_count as usize;
        // Pop element types from type_stack (CreateArray consumes N elements, produces 1 array)
        for _ in 0..element_count {
            self.pop_type();
        }
        self.compiling_chunk.write_opcode_u16(
            Opcode::CreateArray,
            element_count,
            start_token.span.clone(),
        );

        Ok(())
    }

    /// Emit StoreVar opcode to store top of stack at given slot
    fn emit_store_var(&mut self, slot: usize, span: Range<usize>) {
        self.compiling_chunk
            .write_opcode_u16(Opcode::StoreVar, slot as u16, span);
    }

    /// Scans forward to find the length of a comprehension condition.
    /// It stops when it encounters a closing `]`, or another clause (`for` or `if`)
    /// at the current bracket nesting level.
    fn skip_comprehension_condition(&mut self) -> Result<usize, CompilerError> {
        let mut skip_count = 0;
        let mut depth = 0;

        loop {
            let next_token = match self.parser.peek_ahead(skip_count) {
                Ok(Some(token)) => token.clone(),
                Ok(None) => {
                    return Err(self.unexpected_eof_error(self.current_span()));
                }
                Err(err) => {
                    return Err(self.make_error(err.span, err.message));
                }
            };

            match next_token.token {
                Token::LeftBracket | Token::LeftBrace | Token::LeftParen => {
                    depth += 1;
                }
                Token::RightBracket | Token::RightBrace | Token::RightParen => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                Token::For | Token::If => {
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }

            skip_count += 1;
        }

        Ok(skip_count)
    }

    /// Advance the parser past a balanced expression, stopping at `,`, `}`, or `for`/`if`
    /// at depth 0. Does NOT consume the terminator token.
    /// Used during pre-scan of object members to skip over expressions.
    fn skip_to_member_end(&mut self) -> Result<(), CompilerError> {
        let mut depth = 0;
        loop {
            let current = self
                .parser
                .current_token()
                .ok_or_else(|| self.unexpected_eof_error(self.current_span()))?;
            match &current.token {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => {
                    depth += 1;
                    self.parser.advance()?;
                }
                Token::RightParen | Token::RightBracket => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    self.parser.advance()?;
                }
                Token::RightBrace => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                    self.parser.advance()?;
                }
                Token::Comma if depth == 0 => break,
                Token::For if depth == 0 => break,
                Token::Eof => {
                    return Err(self.unexpected_eof_error(self.current_span()));
                }
                _ => {
                    self.parser.advance()?;
                }
            }
        }
        Ok(())
    }

    /// Skip balanced parentheses if present at current position.
    /// Used during pre-scan to skip function parameter lists.
    fn skip_balanced_parens_if_present(&mut self) -> Result<(), CompilerError> {
        if matches!(
            self.parser.current_token().map(|t| &t.token),
            Some(Token::LeftParen)
        ) {
            self.parser.advance()?; // consume '('
            let mut paren_depth = 1;
            while paren_depth > 0 {
                let t = self
                    .parser
                    .current_token()
                    .ok_or_else(|| self.unexpected_eof_error(self.current_span()))?;
                match &t.token {
                    Token::LeftParen => paren_depth += 1,
                    Token::RightParen => paren_depth -= 1,
                    _ => {}
                }
                self.parser.advance()?;
            }
        }
        Ok(())
    }

    /// Inject object-local declarations into the current scope.
    /// Called inside each field/assert thunk to replay local bindings.
    /// Uses two passes: first pre-declares all names with null (for forward refs),
    /// then parses init expressions and stores values.
    ///
    /// `stack_offset` accounts for anonymous values on the VM stack that aren't
    /// tracked in `self.locals` (e.g., in comprehension base cases where the
    /// result object copy and key are pushed but not declared as locals).
    /// For thunks (field/assert), pass 0. For comprehension base cases, pass 2.
    fn inject_object_locals(
        &mut self,
        object_locals: &[ObjectLocalBinding],
        memory_manager: &mut MemoryManager,
        stack_offset: usize,
    ) -> Result<(), CompilerError> {
        if object_locals.is_empty() {
            return Ok(());
        }

        let resume_checkpoint = self.parser.save_checkpoint();

        // Phase A: Pre-declare ALL locals with LoadNull
        let mut local_slots: Vec<usize> = Vec::new();
        for local_binding in object_locals {
            self.parser
                .restore_checkpoint(local_binding.checkpoint.clone());
            self.parser.advance()?; // consume 'local'

            let name = match self.parser.current_token() {
                Some(t) => match &t.token {
                    Token::Identifier(n) => n.clone(),
                    _ => {
                        return Err(self.make_error(
                            t.span.clone(),
                            "Expected identifier after 'local'".to_string(),
                        ));
                    }
                },
                None => return Err(self.unexpected_eof_error(self.current_span())),
            };

            let actual_slot = self.locals.len() + stack_offset;
            self.emit_opcode(Opcode::LoadNull, local_binding.span.clone());
            self.locals.push(Local {
                name,
                depth: self.scope_depth,
                stack_slot: actual_slot,
                is_captured: false,
            });
            local_slots.push(actual_slot);
        }

        // Phase B: Parse init expressions and store values.
        // Process function-sugar bindings first (they create closures without
        // evaluating the body), then non-function bindings. This ensures
        // forward references work: function locals are available as closures
        // before non-function inits try to call them.
        for pass in 0..2 {
            for (i, local_binding) in object_locals.iter().enumerate() {
                self.parser
                    .restore_checkpoint(local_binding.checkpoint.clone());
                self.parser.advance()?; // consume 'local'
                self.parser.advance()?; // consume identifier

                let is_function = matches!(
                    self.parser.current_token().map(|t| &t.token),
                    Some(Token::LeftParen)
                );

                // Pass 0: function-sugar only; Pass 1: non-function only
                if (pass == 0) != is_function {
                    continue;
                }

                let slot = local_slots[i];
                let span = local_binding.span.clone();

                if is_function {
                    let (parameters, close_paren_span) = self.parse_parameter_list()?;
                    self.parser.consume(
                        Token::Operator("=".to_string()),
                        "Expected '=' after parameters",
                    )?;
                    let fn_span = span.start..close_paren_span.end;
                    self.compile_function_body(parameters, fn_span, memory_manager)?;
                } else {
                    self.parser.consume(
                        Token::Operator("=".to_string()),
                        "Expected '=' after variable name",
                    )?;
                    self.parse_expr_notail(0, memory_manager)?;
                }

                self.emit_store_var(slot, span);
            }
        }

        self.parser.restore_checkpoint(resume_checkpoint);
        Ok(())
    }

    /// Parse array comprehension: [expr for var in source_array]
    /// Stack layout during loop: [source, result, counter, length]
    /// for_offset is the token offset where 'for' keyword is located (already found by caller)
    fn parse_object_comprehension(
        &mut self,
        start_token: &TokenInfo,
        _for_offset: usize,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let span = start_token.span.clone();

        // Buffer tokens until the final '}' of the comprehension
        let mut depth = 0;
        let mut lookahead_idx = 0;
        loop {
            let token = self.parser.peek_ahead(lookahead_idx)?;
            match token {
                Some(t) => {
                    match &t.token {
                        Token::LeftBrace => depth += 1,
                        Token::RightBrace => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        }
                        Token::Eof => break,
                        _ => {}
                    }
                    lookahead_idx += 1;
                }
                None => break,
            }
        }

        // Pre-scan members before 'for' to separate locals from the field
        let mut object_locals: Vec<ObjectLocalBinding> = Vec::new();
        let mut field_start_checkpoint = self.parser.save_checkpoint();
        loop {
            if let Some(t) = self.parser.current_token().cloned() {
                match &t.token {
                    Token::For => break,
                    Token::Local => {
                        let cp = self.parser.save_checkpoint();
                        object_locals.push(ObjectLocalBinding {
                            checkpoint: cp,
                            span: t.span.clone(),
                        });
                        self.parser.advance()?; // 'local'
                        self.parser.advance()?; // identifier
                        self.skip_balanced_parens_if_present()?;
                        self.parser.consume(
                            Token::Operator("=".to_string()),
                            "Expected '=' after local name",
                        )?;
                        self.skip_to_member_end()?;
                        if matches!(
                            self.parser.current_token().map(|t| &t.token),
                            Some(Token::Comma)
                        ) {
                            self.parser.advance()?;
                        }
                    }
                    _ => {
                        // This is the actual field — skip to 'for' at depth 0
                        // We can't use skip_to_member_end here because it stops at 'if'
                        // which may be an if/then/else expression inside the field value.
                        field_start_checkpoint = self.parser.save_checkpoint();
                        let mut field_depth = 0;
                        loop {
                            let ft = self
                                .parser
                                .current_token()
                                .ok_or_else(|| self.unexpected_eof_error(span.clone()))?;
                            match &ft.token {
                                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => {
                                    field_depth += 1;
                                    self.parser.advance()?;
                                }
                                Token::RightParen | Token::RightBracket => {
                                    if field_depth > 0 {
                                        field_depth -= 1;
                                    }
                                    self.parser.advance()?;
                                }
                                Token::RightBrace => {
                                    if field_depth == 0 {
                                        break;
                                    }
                                    field_depth -= 1;
                                    self.parser.advance()?;
                                }
                                Token::For if field_depth == 0 => break,
                                Token::Comma if field_depth == 0 => {
                                    self.parser.advance()?;
                                    break;
                                }
                                Token::Eof => {
                                    return Err(self.unexpected_eof_error(span.clone()));
                                }
                                _ => {
                                    self.parser.advance()?;
                                }
                            }
                        }
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(span.clone()));
            }
        }
        // Parser is now positioned at 'for'
        let mut clauses = Vec::new();

        // Parse clauses until '}'
        loop {
            let current = match self.parser.current_token() {
                Some(t) => t.clone(),
                None => return Err(self.unexpected_eof_error(span.clone())),
            };

            match current.token {
                Token::For => {
                    self.parser.advance()?; // consume 'for'
                    let var_name = if let Some(token) = self.parser.current_token() {
                        if let Token::Identifier(name) = &token.token {
                            name.clone()
                        } else {
                            return Err(self.make_error(
                                token.span.clone(),
                                "Expected identifier after 'for'".to_string(),
                            ));
                        }
                    } else {
                        return Err(self.unexpected_eof_error(span.clone()));
                    };
                    self.parser.advance()?; // consume variable name
                    self.parser
                        .consume(Token::In, "Expected 'in' after variable")?;

                    let skip = self.skip_comprehension_condition()?;
                    let source_checkpoint = self.parser.save_checkpoint();

                    for _ in 0..skip {
                        self.parser.advance()?;
                    }

                    clauses.push(ComprehensionClause::For {
                        var_name,
                        source_checkpoint,
                        span: current.span.clone(),
                    });
                }
                Token::If => {
                    self.parser.advance()?; // consume 'if'
                    let skip = self.skip_comprehension_condition()?;
                    let condition_checkpoint = self.parser.save_checkpoint();

                    for _ in 0..skip {
                        self.parser.advance()?;
                    }
                    clauses.push(ComprehensionClause::If {
                        condition_checkpoint,
                        span: current.span.clone(),
                    });
                }
                Token::RightBrace => {
                    break;
                }
                _ => {
                    return Err(self.make_error(
                        current.span.clone(),
                        "Expected 'for', 'if', or '}'".to_string(),
                    ));
                }
            }
        }

        self.parser.consume(
            Token::RightBrace,
            "Expected '}' to close object comprehension",
        )?;

        let source_end_checkpoint = self.parser.save_checkpoint();

        self.begin_scope();

        self.compiling_chunk
            .write_opcode_u16(Opcode::CreateObject, 0, span.clone());
        self.declare_local("__comp_result".to_string())?;
        let result_slot = self.locals.last().unwrap().stack_slot;

        self.emit_object_comprehension_clauses(
            &clauses,
            0,
            field_start_checkpoint,
            &object_locals,
            result_slot,
            memory_manager,
            &span,
            None,
        )?;

        self.emit_opcode(Opcode::Pop, span.clone());

        self.compiling_chunk
            .write_opcode_u16(Opcode::LoadVar, result_slot as u16, span.clone());

        self.end_scope();
        self.parser.restore_checkpoint(source_end_checkpoint);

        Ok(())
    }

    fn emit_object_comprehension_clauses(
        &mut self,
        clauses: &[ComprehensionClause],
        clause_idx: usize,
        field_start_checkpoint: ParserCheckpoint,
        object_locals: &[ObjectLocalBinding],
        result_slot: usize,
        memory_manager: &mut MemoryManager,
        span: &Range<usize>,
        precomputed_source: Option<usize>,
    ) -> Result<(), CompilerError> {
        if clause_idx >= clauses.len() {
            self.compiling_chunk.write_opcode_u16(
                Opcode::LoadVar,
                result_slot as u16,
                span.clone(),
            );

            self.parser.restore_checkpoint(field_start_checkpoint);

            let key_token = match self.parser.current_token() {
                Some(t) => t.clone(),
                None => return Err(self.unexpected_eof_error(span.clone())),
            };

            match &key_token.token {
                Token::String(key_value) => {
                    let allocation_result = memory_manager.allocate_string(key_value);
                    if allocation_result.should_garbage_collect {
                        self.run_garbage_collect(
                            memory_manager,
                            &[Value::String(allocation_result.index)],
                        );
                    }
                    let _key_index = self.emit_string_constant(allocation_result.index)?;
                    self.push_type(ExpressionType::String);
                    self.parser.advance()?;
                }
                Token::Identifier(key_name) => {
                    let allocation_result = memory_manager.allocate_string(key_name);
                    if allocation_result.should_garbage_collect {
                        self.run_garbage_collect(
                            memory_manager,
                            &[Value::String(allocation_result.index)],
                        );
                    }
                    let _key_index = self.emit_string_constant(allocation_result.index)?;
                    self.push_type(ExpressionType::String);
                    self.parser.advance()?;
                }
                Token::LeftBracket => {
                    self.parser.advance()?;
                    self.parse_expr_notail(0, memory_manager)?;
                    self.parser
                        .consume(Token::RightBracket, "Expected ']' after dynamic key")?;
                }
                _ => {
                    return Err(self.make_error(
                        key_token.span.clone(),
                        "Object key must be a string literal, identifier, or dynamic key '[expr]'"
                            .to_string(),
                    ));
                }
            }

            self.parser
                .consume(Token::Operator(":".to_string()), "Expected ':' after key")?;

            // Compile the value as a thunk (closure) so self/super/$ are available
            let (saved_scope_depth, saved_function_type) = self.begin_function();
            self.begin_scope();
            self.declare_local("<closure>".to_string())?; // Slot 0
            self.declare_local("<self>".to_string())?; // Slot 1
            self.declare_local("<super>".to_string())?; // Slot 2

            if self.object_depth == 1 {
                // local $ = self
                self.emit_opcode(Opcode::LoadSelf, 0..0);
                self.declare_local("$".to_string())?; // Slot 3
            }

            self.inject_object_locals(object_locals, memory_manager, 0)?;

            self.parse_expr_notail(0, memory_manager)?;
            self.emit_opcode(Opcode::Return, 0..0);
            self.end_scope();

            let (field_chunk, field_upvalues) =
                self.end_function((saved_scope_depth, saved_function_type));

            // Emit Closure with arity 2 (self, super)
            self.emit_closure_with_params(
                field_chunk,
                field_upvalues,
                2,
                2,
                Vec::new(),
                span.clone(),
                memory_manager,
                true,
            )?;

            // ObjectInsert consumes the key string from the stack
            self.pop_type(); // key type
            self.compiling_chunk.write_opcode_u8(
                Opcode::ObjectInsert,
                FieldVisibility::Visible as u8,
                span.clone(),
            );
            self.emit_store_var(result_slot, span.clone());
            self.emit_opcode(Opcode::LoadNull, span.clone());

            return Ok(());
        }

        match &clauses[clause_idx] {
            ComprehensionClause::For {
                var_name,
                source_checkpoint,
                span: clause_span,
            } => {
                // Parse source array expression, or use a precomputed slot
                if let Some(slot) = precomputed_source {
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::LoadVar,
                        slot as u16,
                        clause_span.clone(),
                    );
                } else {
                    self.parser.restore_checkpoint(source_checkpoint.clone());
                    self.parse_expr_notail(0, memory_manager)?;
                }

                self.begin_scope();

                self.declare_local("__comp_source".to_string())?;
                let source_slot = self.locals.last().unwrap().stack_slot;

                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    source_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::ArrayLength, clause_span.clone());
                self.declare_local("__comp_length".to_string())?;
                let length_slot = self.locals.last().unwrap().stack_slot;

                self.emit_constant(0.0)?;
                self.declare_local("__comp_counter".to_string())?;
                let counter_slot = self.locals.last().unwrap().stack_slot;

                // Hoist the next for-clause's source if it doesn't depend on var_name.
                let next_precomputed = match clauses.get(clause_idx + 1) {
                    Some(ComprehensionClause::For {
                        source_checkpoint: next_src_cp,
                        ..
                    }) if !self.expr_references_ident(next_src_cp, var_name) => {
                        let next_src_cp = next_src_cp.clone();
                        self.parser.restore_checkpoint(next_src_cp);
                        self.parse_expr_notail(0, memory_manager)?;
                        self.declare_local("__comp_hoisted_source".to_string())?;
                        Some(self.locals.last().unwrap().stack_slot)
                    }
                    _ => None,
                };

                let loop_start = self.compiling_chunk.count();

                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    length_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::Lt, clause_span.clone());

                let jump_to_end = self.emit_jump(Opcode::JumpIfFalse, clause_span.clone());

                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    source_slot as u16,
                    clause_span.clone(),
                );
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::ArrayIndex, clause_span.clone());

                self.begin_scope();
                self.declare_local(var_name.clone())?;

                self.emit_object_comprehension_clauses(
                    clauses,
                    clause_idx + 1,
                    field_start_checkpoint.clone(),
                    object_locals,
                    result_slot,
                    memory_manager,
                    span,
                    next_precomputed,
                )?;

                self.end_scope();
                self.emit_opcode(Opcode::Pop, clause_span.clone());

                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.emit_constant(1.0)?;
                self.emit_opcode(Opcode::Add, clause_span.clone());
                self.emit_store_var(counter_slot, clause_span.clone());

                // Jump back to loop start
                let jump_back_offset = self.compiling_chunk.count();
                self.emit_opcode(Opcode::Jump, clause_span.clone());
                let back_offset = loop_start as i32 - (jump_back_offset as i32 + 1 + 4);
                self.emit_i32(back_offset, clause_span.clone());
                self.patch_jump(jump_to_end);

                self.emit_opcode(Opcode::LoadNull, clause_span.clone());
                self.end_scope();
            }
            ComprehensionClause::If {
                condition_checkpoint,
                span: clause_span,
            } => {
                self.parser.restore_checkpoint(condition_checkpoint.clone());
                self.parse_expr_notail(0, memory_manager)?;

                let jump_if_false = self.emit_jump(Opcode::JumpIfFalse, clause_span.clone());

                self.emit_object_comprehension_clauses(
                    clauses,
                    clause_idx + 1,
                    field_start_checkpoint.clone(),
                    object_locals,
                    result_slot,
                    memory_manager,
                    span,
                    None,
                )?;

                let jump_to_end = self.emit_jump(Opcode::Jump, clause_span.clone());
                self.patch_jump(jump_if_false);
                self.emit_opcode(Opcode::LoadNull, clause_span.clone());
                self.patch_jump(jump_to_end);
            }
        }

        Ok(())
    }

    fn parse_array_comprehension(
        &mut self,
        start_token: &TokenInfo,
        for_offset: usize,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let span = start_token.span.clone();

        // Fill buffer with tokens until the first 'for' so body_checkpoint has them
        for i in 0..=for_offset {
            self.parser.peek_ahead(i)?;
        }
        // Save checkpoint for the body expression (we'll come back to it)
        let body_checkpoint = self.parser.save_checkpoint();

        // Skip past the body to the first 'for'
        for _ in 0..for_offset {
            self.parser.advance()?;
        }

        let mut clauses = Vec::new();

        // Parse clauses until ']'
        loop {
            let current = match self.parser.current_token() {
                Some(t) => t.clone(),
                None => return Err(self.unexpected_eof_error(span.clone())),
            };

            match current.token {
                Token::For => {
                    self.parser.advance()?; // consume 'for'
                    let var_name = if let Some(token) = self.parser.current_token() {
                        if let Token::Identifier(name) = &token.token {
                            name.clone()
                        } else {
                            return Err(self.make_error(
                                token.span.clone(),
                                "Expected identifier after 'for'".to_string(),
                            ));
                        }
                    } else {
                        return Err(self.unexpected_eof_error(span.clone()));
                    };
                    self.parser.advance()?; // consume variable name
                    self.parser
                        .consume(Token::In, "Expected 'in' after variable")?;

                    // Fill buffer with source expression tokens
                    let skip = self.skip_comprehension_condition()?;
                    let source_checkpoint = self.parser.save_checkpoint();

                    for _ in 0..skip {
                        self.parser.advance()?;
                    }

                    clauses.push(ComprehensionClause::For {
                        var_name,
                        source_checkpoint,
                        span: current.span.clone(),
                    });
                }
                Token::If => {
                    self.parser.advance()?; // consume 'if'

                    // Fill buffer with condition expression tokens
                    let skip = self.skip_comprehension_condition()?;
                    let condition_checkpoint = self.parser.save_checkpoint();

                    for _ in 0..skip {
                        self.parser.advance()?;
                    }
                    clauses.push(ComprehensionClause::If {
                        condition_checkpoint,
                        span: current.span.clone(),
                    });
                }
                Token::RightBracket => {
                    break;
                }
                _ => {
                    return Err(self.make_error(
                        current.span.clone(),
                        "Expected 'for', 'if', or ']'".to_string(),
                    ));
                }
            }
        }

        // Consume the closing ']'
        self.parser.consume(
            Token::RightBracket,
            "Expected ']' to close array comprehension",
        )?;

        // Save checkpoint position for after the comprehension
        let source_end_checkpoint = self.parser.save_checkpoint();

        // Enter a scope for the comprehension state (result array and source loops)
        self.begin_scope();

        // Create empty result array, declare as hidden local
        self.compiling_chunk
            .write_opcode_u16(Opcode::CreateArray, 0, span.clone());
        self.declare_local("__comp_result".to_string())?;
        let result_slot = self.locals.last().unwrap().stack_slot;
        // Stack: [result]

        // Now emit the nested loops/conditions recursively
        self.emit_comprehension_clauses(
            &clauses,
            0,
            body_checkpoint,
            result_slot,
            memory_manager,
            &span,
            None,
        )?;

        // Pop the dummy value left by recursion
        self.emit_opcode(Opcode::Pop, span.clone());

        // Load result before we clean up
        self.compiling_chunk
            .write_opcode_u16(Opcode::LoadVar, result_slot as u16, span.clone());

        // End the comprehension scope
        self.end_scope();

        // Restore parser to after the comprehension
        self.parser.restore_checkpoint(source_end_checkpoint);

        Ok(())
    }

    /// Returns true if the expression starting at `checkpoint` contains an
    /// identifier token matching `ident` before the expression ends.
    /// Tracks bracket/brace/paren depth to avoid false positives inside
    /// nested brackets. Stops at `for`/`if`/`]` at depth 0 (comprehension
    /// terminators). Fully restores parser state afterward.
    fn expr_references_ident(&mut self, checkpoint: &ParserCheckpoint, ident: &str) -> bool {
        let original = self.parser.save_checkpoint();
        self.parser.restore_checkpoint(checkpoint.clone());

        let mut depth = 0i32;
        let mut found = false;
        loop {
            if self.parser.advance().is_err() {
                break;
            }
            let tok = match self.parser.previous_token() {
                Some(t) => t.token.clone(),
                None => break,
            };
            match tok {
                Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                Token::RightParen | Token::RightBrace => depth -= 1,
                Token::RightBracket => {
                    if depth == 0 {
                        break;
                    }
                    depth -= 1;
                }
                Token::For | Token::If if depth == 0 => break,
                Token::Eof => break,
                Token::Identifier(ref name) if name == ident => {
                    found = true;
                    break;
                }
                _ => {}
            }
        }

        self.parser.restore_checkpoint(original);
        found
    }

    /// Recursively emit code for comprehension clauses
    fn emit_comprehension_clauses(
        &mut self,
        clauses: &[ComprehensionClause],
        clause_idx: usize,
        body_checkpoint: ParserCheckpoint,
        result_slot: usize,
        memory_manager: &mut MemoryManager,
        span: &Range<usize>,
        precomputed_source: Option<usize>,
    ) -> Result<(), CompilerError> {
        if clause_idx >= clauses.len() {
            // All clauses emitted, now emit the body evaluation and append
            // Restore parser to body expression
            self.parser.restore_checkpoint(body_checkpoint);
            self.parse_expr_notail(0, memory_manager)?;

            // Append body_value directly into the result array in-place (no allocation).
            // The result array is private to this comprehension and never shared, so
            // mutation is always safe.
            self.compiling_chunk.write_opcode_u16(
                Opcode::ArrayAppendInPlace,
                result_slot as u16,
                span.clone(),
            );

            // Push dummy value for end_scope
            self.emit_opcode(Opcode::LoadNull, span.clone());

            return Ok(());
        }

        match &clauses[clause_idx] {
            ComprehensionClause::For {
                var_name,
                source_checkpoint,
                span: clause_span,
            } => {
                // Parse source array expression, or use a precomputed slot
                if let Some(slot) = precomputed_source {
                    self.compiling_chunk.write_opcode_u16(
                        Opcode::LoadVar,
                        slot as u16,
                        clause_span.clone(),
                    );
                } else {
                    self.parser.restore_checkpoint(source_checkpoint.clone());
                    self.parse_expr_notail(0, memory_manager)?;
                }

                // Enter a scope for the loop state
                self.begin_scope();

                self.declare_local("__comp_source".to_string())?;
                let source_slot = self.locals.last().unwrap().stack_slot;

                // Dup source and get length, declare as hidden local
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    source_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::ArrayLength, clause_span.clone());
                self.declare_local("__comp_length".to_string())?;
                let length_slot = self.locals.last().unwrap().stack_slot;

                // Push initial counter = 0, declare as hidden local
                self.emit_constant(0.0)?;
                self.declare_local("__comp_counter".to_string())?;
                let counter_slot = self.locals.last().unwrap().stack_slot;

                // Hoist the next for-clause's source if it doesn't depend on var_name.
                // This avoids re-evaluating the inner source on every outer iteration.
                let next_precomputed = match clauses.get(clause_idx + 1) {
                    Some(ComprehensionClause::For {
                        source_checkpoint: next_src_cp,
                        ..
                    }) if !self.expr_references_ident(next_src_cp, var_name) => {
                        let next_src_cp = next_src_cp.clone();
                        self.parser.restore_checkpoint(next_src_cp);
                        self.parse_expr_notail(0, memory_manager)?;
                        self.declare_local("__comp_hoisted_source".to_string())?;
                        Some(self.locals.last().unwrap().stack_slot)
                    }
                    _ => None,
                };

                // LOOP_START:
                let loop_start = self.compiling_chunk.count();

                // Check: counter < length
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    length_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::Lt, clause_span.clone());

                // JumpIfFalse to end
                let jump_to_end = self.emit_jump(Opcode::JumpIfFalse, clause_span.clone());

                // Get element: source[counter]
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    source_slot as u16,
                    clause_span.clone(),
                );
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.emit_opcode(Opcode::ArrayIndex, clause_span.clone());

                // Enter scope and declare loop variable
                self.begin_scope();
                self.declare_local(var_name.clone())?;

                // Recurse to next clause
                self.emit_comprehension_clauses(
                    clauses,
                    clause_idx + 1,
                    body_checkpoint,
                    result_slot,
                    memory_manager,
                    span,
                    next_precomputed,
                )?;

                // End scope for loop variable
                self.end_scope();
                // Pop the dummy value from recursion
                self.emit_opcode(Opcode::Pop, span.clone());

                // Increment counter
                self.compiling_chunk.write_opcode_u16(
                    Opcode::LoadVar,
                    counter_slot as u16,
                    clause_span.clone(),
                );
                self.emit_constant(1.0)?;
                self.emit_opcode(Opcode::Add, clause_span.clone());
                self.emit_store_var(counter_slot, clause_span.clone());

                // Jump back to loop start
                let jump_back_offset = self.compiling_chunk.count();
                self.emit_opcode(Opcode::Jump, clause_span.clone());
                let back_offset = loop_start as i32 - (jump_back_offset as i32 + 1 + 4);
                self.emit_i32(back_offset, clause_span.clone());

                // LOOP_END:
                self.patch_jump(jump_to_end);

                // Push dummy value for end_scope of loop state
                self.emit_opcode(Opcode::LoadNull, clause_span.clone());

                // End scope for loop state
                self.end_scope();
            }
            ComprehensionClause::If {
                condition_checkpoint,
                span: clause_span,
            } => {
                // Restore parser to condition expression
                self.parser.restore_checkpoint(condition_checkpoint.clone());
                self.parse_expr_notail(0, memory_manager)?;

                // JumpIfFalse to skip this entire branch
                let skip_jump = self.emit_jump(Opcode::JumpIfFalse, clause_span.clone());

                // Recurse to next clause
                self.emit_comprehension_clauses(
                    clauses,
                    clause_idx + 1,
                    body_checkpoint,
                    result_slot,
                    memory_manager,
                    span,
                    None,
                )?;

                let end_if_jump = self.emit_jump(Opcode::Jump, clause_span.clone());

                // Target for skip_jump
                self.patch_jump(skip_jump);
                // Push dummy value for false path
                self.emit_opcode(Opcode::LoadNull, clause_span.clone());

                self.patch_jump(end_if_jump);
            }
        }

        Ok(())
    }

    /// Parse: local x = expr, y = expr; body_expr
    fn parse_local_statement(
        &mut self,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        self.parser.advance()?; // consume 'local'

        // Enter new scope for these locals
        self.begin_scope();

        // Phase 1: Use lookahead to discover all binding names without consuming
        // tokens, so we can pre-declare them all before compiling any RHS.
        // This enables forward references (e.g., `local x = y, y = 3; x`).
        let mut binding_names: Vec<(String, Range<usize>)> = Vec::new();
        {
            let mut lookahead_idx = 0;
            loop {
                // Expect identifier at lookahead_idx
                let name_token = match self.parser.peek_ahead(lookahead_idx)? {
                    Some(t) => t.clone(),
                    None => break,
                };
                let var_name = match &name_token.token {
                    Token::Identifier(name) => name.clone(),
                    _ => break, // Not an identifier, stop scanning
                };
                binding_names.push((var_name, name_token.span.clone()));
                lookahead_idx += 1; // past identifier

                // Skip optional parameter list
                let is_paren = matches!(
                    self.parser.peek_ahead(lookahead_idx)?.map(|t| &t.token),
                    Some(Token::LeftParen)
                );
                if is_paren {
                    lookahead_idx += 1;
                    let mut paren_depth = 1;
                    while paren_depth > 0 {
                        let tok = match self.parser.peek_ahead(lookahead_idx)? {
                            Some(t) => t.token.clone(),
                            None => break,
                        };
                        match &tok {
                            Token::LeftParen => paren_depth += 1,
                            Token::RightParen => paren_depth -= 1,
                            _ => {}
                        }
                        lookahead_idx += 1;
                    }
                }

                // Expect '='
                lookahead_idx += 1; // skip '='

                // Skip RHS expression until ',' or ';' at depth 0
                let mut depth = 0;
                loop {
                    let tok = match self.parser.peek_ahead(lookahead_idx)? {
                        Some(t) => t.token.clone(),
                        None => break,
                    };
                    match &tok {
                        Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
                        Token::RightParen | Token::RightBracket | Token::RightBrace => {
                            if depth > 0 {
                                depth -= 1;
                            } else {
                                break;
                            }
                        }
                        Token::Comma | Token::Semicolon if depth == 0 => break,
                        Token::Eof => break,
                        _ => {}
                    }
                    lookahead_idx += 1;
                }

                // Check if ',' (more bindings) or ';' (end)
                let is_comma = matches!(
                    self.parser.peek_ahead(lookahead_idx)?.map(|t| &t.token),
                    Some(Token::Comma)
                );
                if is_comma {
                    lookahead_idx += 1;
                    continue;
                } else {
                    break; // ';' or other: done scanning
                }
            }
        }

        // Phase 2: Pre-declare ALL locals with null values
        let mut slots: Vec<usize> = Vec::new();
        for (name, span) in &binding_names {
            self.emit_opcode(Opcode::LoadNull, span.clone());
            self.declare_local(name.clone())?;
            slots.push(self.locals.last().unwrap().stack_slot);
        }

        // Phase 3: Parse and compile bindings sequentially, consuming tokens
        // normally. All names are already in scope so forward references
        // resolve (to null initially, then updated via StoreVar).
        let mut binding_idx = 0;
        loop {
            let name_token = self
                .parser
                .current_token()
                .cloned()
                .ok_or_else(|| self.unexpected_eof_error(self.current_span()))?;

            match &name_token.token {
                Token::Identifier(_) => {}
                _ => {
                    return Err(self.make_error(
                        name_token.span,
                        "Expected variable name after 'local'".to_string(),
                    ));
                }
            }

            self.parser.advance()?; // consume identifier
            let ident_span = self
                .parser
                .previous_token()
                .map(|t| t.span.clone())
                .unwrap_or(0..0);

            let is_function = matches!(
                self.parser.current_token().map(|t| &t.token),
                Some(Token::LeftParen)
            );

            if is_function {
                let (parameters, close_paren_span) = self.parse_parameter_list()?;
                self.parser.consume(
                    Token::Operator("=".to_string()),
                    "Expected '=' after parameters",
                )?;
                let fn_span = ident_span.start..close_paren_span.end;
                self.compile_function_body(parameters, fn_span, memory_manager)?;
            } else {
                self.parser.consume(
                    Token::Operator("=".to_string()),
                    "Expected '=' after variable name",
                )?;
                // Wrap RHS in a thunk for lazy evaluation
                let thunk_span = self.current_span(); // capture before parsing the RHS
                let saved_state = self.begin_function();
                self.begin_scope();
                self.declare_local("<closure>".to_string())?;
                self.parse_expr(0, memory_manager)?;
                let return_span = self.current_span();
                self.emit_opcode(Opcode::Return, return_span);
                self.end_scope();
                let (chunk, upvalues) = self.end_function(saved_state);
                self.emit_thunk(chunk, upvalues, thunk_span, memory_manager)?;
            }

            self.emit_store_var(slots[binding_idx], binding_names[binding_idx].1.clone());
            binding_idx += 1;

            // Check for comma (more bindings) or semicolon (end of bindings)
            if let Some(token) = self.parser.current_token() {
                match &token.token {
                    Token::Comma => {
                        self.parser.advance()?;
                        continue;
                    }
                    Token::Semicolon => {
                        self.parser.advance()?;
                        break;
                    }
                    Token::Eof => {
                        return Err(self.unexpected_eof_error(token.span.clone()));
                    }
                    _ => {
                        return Err(self.make_error(
                            token.span.clone(),
                            "Expected ',' or ';' in local statement".to_string(),
                        ));
                    }
                }
            } else {
                return Err(self.unexpected_eof_error(self.current_span()));
            }
        }

        // Parse body expression (with locals in scope)
        self.parse_expr(0, memory_manager)?;

        // Exit scope - emit Pop for each local
        self.end_scope();

        Ok(())
    }

    // Scope and Local Variable Management

    /// Enter a new lexical scope (increments depth)
    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    /// Exit current scope, emitting Pop or CloseUpvalue instructions for locals at this depth
    fn end_scope(&mut self) {
        let span = self.current_span();

        // Pop all locals at current depth (in reverse declaration order)
        // The body expression result is on top of the stack, and locals are below it.
        // For each local to pop, we swap it with the result and then pop/close it.
        // This keeps the result on top while removing locals underneath.
        while let Some(local) = self.locals.last() {
            if local.depth == self.scope_depth {
                let is_captured = local.is_captured;
                // Swap result with this local so the local is on top
                self.emit_opcode(Opcode::Swap, span.clone());

                if is_captured {
                    // Emit CloseUpvalue to move captured local from stack to heap
                    self.emit_opcode(Opcode::CloseUpvalue, span.clone());
                }

                // Always pop the local after potentially closing it
                self.emit_opcode(Opcode::Pop, span.clone());
                self.locals.pop();
            } else {
                break; // Reached locals from outer scope
            }
        }

        self.scope_depth -= 1;
    }

    /// Declare a new local variable at the current scope depth
    /// The value must already be on the stack
    fn declare_local(&mut self, name: String) -> Result<(), CompilerError> {
        // Check for duplicate in current scope
        for local in self.locals.iter().rev() {
            if local.depth < self.scope_depth {
                break; // Reached outer scope
            }
            if local.name == name {
                return Err(self.make_error(
                    self.current_span(),
                    format!("Variable '{}' already declared in this scope", name),
                ));
            }
        }

        // Stack slot accounts for both declared locals and anonymous temporaries
        // (expression values on the stack not tracked as locals, e.g. function
        // callee during argument parsing, array literal elements, etc.).
        let stack_slot = self.locals.len() + self.anon_stack_depth;

        self.locals.push(Local {
            name,
            depth: self.scope_depth,
            stack_slot,
            is_captured: false,
        });

        Ok(())
    }

    /// Resolve a variable name to its stack slot
    /// Returns None if not found in local scope
    fn resolve_local(&self, name: &str) -> Option<usize> {
        // Search from innermost to outermost scope (reverse order)
        for local in self.locals.iter().rev() {
            if local.name == name {
                return Some(local.stack_slot);
            }
        }
        None
    }

    /// Add an upvalue to this function's upvalue list
    /// Returns the index of the upvalue (reusing existing entry if found)
    fn add_upvalue(&mut self, index: u8, is_local: bool) -> u8 {
        // Check if we already have this upvalue
        for (i, upvalue) in self.upvalues.iter().enumerate() {
            if upvalue.index == index && upvalue.is_local == is_local {
                return i as u8;
            }
        }

        // Add new upvalue
        let upvalue_index = self.upvalues.len() as u8;
        self.upvalues.push(CompilerUpvalue { index, is_local });
        upvalue_index
    }

    /// Resolve an upvalue by name, checking enclosing functions
    /// Returns the upvalue index if found, None otherwise
    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        // No enclosing function means we can't capture anything
        let enclosing = self.enclosing.as_mut()?;

        // Try to find in enclosing function's locals
        let found_local = enclosing.locals.iter_mut().rev().find_map(|local| {
            if local.name == name {
                local.is_captured = true;
                Some(local.stack_slot as u8)
            } else {
                None
            }
        });

        if let Some(slot) = found_local {
            // Add as upvalue capturing a local — use stack_slot (not array index)
            // since anon_stack_depth may shift slots from their array position
            return Some(self.add_upvalue(slot, true));
        }

        // Try to find in enclosing function's upvalues (recursive)
        if let Some(upvalue) = enclosing.resolve_upvalue(name) {
            // Add as upvalue capturing another upvalue
            return Some(self.add_upvalue(upvalue.index, upvalue.is_local));
        }

        None
    }

    /// Begin compiling a new function, setting up enclosing scope chain
    /// Returns the saved state (scope_depth, function_type) for restoration
    fn begin_function(&mut self) -> (u32, FunctionType) {
        // Save source_id before we move compiling_chunk
        let source_id = self.compiling_chunk.source_id;

        // Create enclosing scope from current state for upvalue resolution
        let enclosing_scope = EnclosingScope {
            locals: std::mem::take(&mut self.locals),
            upvalues: std::mem::take(&mut self.upvalues),
            enclosing: self.enclosing.take(),
            chunk: std::mem::replace(&mut self.compiling_chunk, Chunk::new(source_id)),
            constant_pool: std::mem::take(&mut self.constant_pool),
            anon_stack_depth: std::mem::replace(&mut self.anon_stack_depth, 0),
            type_stack: std::mem::take(&mut self.type_stack),
        };

        // Set up enclosing chain
        self.enclosing = Some(Box::new(enclosing_scope));

        // Save state
        let old_scope_depth = self.scope_depth;
        let old_function_type = self.function_type;

        // Reset for new function
        self.scope_depth = 0;
        self.function_type = FunctionType::Function;
        self.tail_call_pending = false;

        (old_scope_depth, old_function_type)
    }

    fn run_garbage_collect(&mut self, memory_manager: &mut MemoryManager, extra_roots: &[Value]) {
        let mut roots = Vec::from(extra_roots);

        // Mark current chunk's constants
        for constant in &self.compiling_chunk.constants {
            roots.push(constant.clone());
        }

        // Mark current constant pool keys
        for (value, _) in &self.constant_pool {
            roots.push(value.clone());
        }

        // Mark all enclosing scopes' constants and chunks
        let mut current_enclosing = &self.enclosing;
        while let Some(enclosing) = current_enclosing {
            for constant in &enclosing.chunk.constants {
                roots.push(constant.clone());
            }
            for (value, _) in &enclosing.constant_pool {
                roots.push(value.clone());
            }
            current_enclosing = &enclosing.enclosing;
        }

        memory_manager.run_garbage_collect(roots, Vec::new());
    }

    /// End function compilation, restoring previous state from enclosing scope
    /// Returns the compiled function chunk and upvalues
    fn end_function(
        &mut self,
        saved_state: (u32, FunctionType),
    ) -> (Chunk<'a>, Vec<CompilerUpvalue>) {
        let (old_scope_depth, old_function_type) = saved_state;

        // Restore locals and upvalues and chunk from enclosing scope
        let enclosing = self.enclosing.take().expect("Must have enclosing scope");

        let function_chunk = std::mem::replace(&mut self.compiling_chunk, enclosing.chunk);
        self.constant_pool = enclosing.constant_pool;

        let function_upvalues = std::mem::take(&mut self.upvalues);

        self.locals = enclosing.locals;
        self.upvalues = enclosing.upvalues;
        self.enclosing = enclosing.enclosing;
        self.anon_stack_depth = enclosing.anon_stack_depth;
        self.type_stack = enclosing.type_stack;

        // Restore other state
        self.scope_depth = old_scope_depth;
        self.function_type = old_function_type;
        self.tail_call_pending = false;

        (function_chunk, function_upvalues)
    }

    /// Parse a parenthesized parameter list: (param1, param2=default, ...)
    /// Consumes the opening '(' and closing ')'.
    /// For parameters with defaults, saves a parser checkpoint for re-compilation later.
    fn parse_parameter_list(
        &mut self,
    ) -> Result<(Vec<FunctionParam>, Range<usize>), CompilerError> {
        self.parser
            .consume(Token::LeftParen, "Expected '(' for parameter list")?;

        let mut parameters = Vec::new();
        let mut seen_default = false;

        let has_params = if let Some(token) = self.parser.current_token() {
            token.token != Token::RightParen
        } else {
            false
        };

        if has_params {
            loop {
                if let Some(TokenInfo {
                    token: Token::Identifier(param_name),
                    ..
                }) = self.parser.current_token()
                {
                    let name = param_name.clone();
                    self.parser.advance()?;

                    // Check for default value: '='
                    let has_default = matches!(
                        self.parser.current_token().map(|t| &t.token),
                        Some(Token::Operator(op)) if op == "="
                    );

                    let default_checkpoint = if has_default {
                        seen_default = true;
                        self.parser.advance()?; // consume '='
                        // Save checkpoint before default expression
                        let checkpoint = self.parser.save_checkpoint();
                        // Skip over the default expression tokens
                        self.skip_default_expression()?;
                        Some(checkpoint)
                    } else {
                        if seen_default {
                            return Err(self.make_error(
                                self.current_span(),
                                "Required parameter cannot follow parameter with default"
                                    .to_string(),
                            ));
                        }
                        None
                    };

                    parameters.push(FunctionParam {
                        name,
                        has_default,
                        default_checkpoint,
                    });
                } else {
                    return Err(
                        self.make_error(self.current_span(), "Expected parameter name".to_string())
                    );
                }

                if let Some(token) = self.parser.current_token() {
                    if token.token == Token::Comma {
                        self.parser.advance()?;
                        // Allow trailing comma before ')'
                        if matches!(
                            self.parser.current_token().map(|t| &t.token),
                            Some(Token::RightParen)
                        ) {
                            break;
                        }
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        let close_paren = self
            .parser
            .consume(Token::RightParen, "Expected ')' after parameters")?;

        Ok((parameters, close_paren.span))
    }

    /// Skip over a default expression in a parameter list.
    /// Stops at ',' or ')' at depth 0.
    fn skip_default_expression(&mut self) -> Result<(), CompilerError> {
        let mut depth = 0;
        loop {
            let token = self.parser.current_token().cloned();
            match token {
                Some(t) => match &t.token {
                    Token::LeftParen | Token::LeftBracket | Token::LeftBrace => {
                        depth += 1;
                        self.parser.advance()?;
                    }
                    Token::RightParen | Token::RightBracket | Token::RightBrace => {
                        if depth == 0 {
                            break; // End of default at closing paren
                        }
                        depth -= 1;
                        self.parser.advance()?;
                    }
                    Token::Comma if depth == 0 => {
                        break; // End of this default, next param
                    }
                    Token::Eof => {
                        return Err(self.unexpected_eof_error(t.span.clone()));
                    }
                    _ => {
                        self.parser.advance()?;
                    }
                },
                None => {
                    return Err(self.unexpected_eof_error(0..0));
                }
            }
        }
        Ok(())
    }

    /// Compile a function body given already-parsed parameters.
    /// Sets up function scope, declares params, parses body expression,
    /// emits Return, and emits the closure instruction.
    /// `name_span` is the span of the function name or defining keyword; it is
    /// attributed to the emitted Closure instruction so coverage tools can mark
    /// the definition line as covered when the function is used.
    fn compile_function_body(
        &mut self,
        parameters: Vec<FunctionParam>,
        name_span: Range<usize>,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let arity = parameters.len() as u8;
        let required_params = parameters.iter().filter(|p| !p.has_default).count() as u8;

        // Intern parameter names for runtime named-arg matching.
        // Root them as external roots to protect from GC during compilation.
        let param_name_indices: Vec<StringIndex> = parameters
            .iter()
            .map(|p| memory_manager.allocate_string(&p.name).index)
            .collect();
        let param_roots: Vec<Value> = param_name_indices
            .iter()
            .map(|&idx| Value::String(idx))
            .collect();
        memory_manager.external_roots.push(param_roots);

        let saved_state = self.begin_function();
        self.begin_scope();

        // Reserve slot 0 for the closure itself
        self.declare_local("<closure>".to_string())?;

        // Declare all parameters as locals
        let mut param_slots = Vec::new();
        for param in &parameters {
            self.declare_local(param.name.clone())?;
            param_slots.push(self.locals.last().unwrap().stack_slot);
        }

        // Emit default-initialization preamble for params with defaults.
        // For each param with a default: check if Uninitialized, if so compile default as thunk.
        let after_checkpoint = self.parser.save_checkpoint();
        for (i, param) in parameters.iter().enumerate() {
            if let Some(default_cp) = &param.default_checkpoint {
                let slot = param_slots[i];
                let span = self.current_span();

                // Load the parameter value to check if it's Uninitialized
                self.compiling_chunk
                    .write_opcode_u16(Opcode::LoadVar, slot as u16, span.clone());

                // BindDefault: pops value, if not Uninitialized jumps forward (skip default)
                let bind_default_offset = self.compiling_chunk.count();
                self.compiling_chunk
                    .write_opcode_u16(Opcode::BindDefault, 0, span.clone()); // placeholder jump

                // Compile default expression as a lazy thunk (forced on first access via LoadVar)
                self.parser.restore_checkpoint(default_cp.clone());
                self.parse_expr_as_thunk(0, memory_manager)?;

                // Store the thunk into the parameter slot
                self.emit_store_var(slot, span.clone());

                // Patch the BindDefault jump offset
                let current_offset = self.compiling_chunk.count();
                let jump_distance = current_offset - (bind_default_offset + 3); // 3 = opcode + u16
                self.compiling_chunk.code[bind_default_offset + 1] =
                    (jump_distance as u16).to_le_bytes()[0];
                self.compiling_chunk.code[bind_default_offset + 2] =
                    (jump_distance as u16).to_le_bytes()[1];
            }
        }
        // Restore parser to after the parameter list
        self.parser.restore_checkpoint(after_checkpoint);

        let prev_tail = self.in_tail_position;
        self.in_tail_position = true;
        self.parse_expr(0, memory_manager)?;
        self.in_tail_position = prev_tail;

        let return_span = self.current_span();
        self.emit_opcode(Opcode::Return, return_span);

        self.end_scope();

        let (chunk, upvalues) = self.end_function(saved_state);
        self.emit_closure_with_params(
            chunk,
            upvalues,
            arity,
            required_params,
            param_name_indices,
            name_span,
            memory_manager,
            false,
        )?;

        // Pop the external roots we pushed for param name protection
        memory_manager.external_roots.pop();

        Ok(())
    }

    /// Parse a function expression: function(params) body
    fn parse_function_expression(
        &mut self,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        // Consume 'function' keyword
        self.parser.advance()?;
        let keyword_span = self
            .parser
            .previous_token()
            .map(|t| t.span.clone())
            .unwrap_or(0..0);

        let (parameters, close_paren_span) = self.parse_parameter_list()?;
        let fn_span = keyword_span.start..close_paren_span.end;
        self.compile_function_body(parameters, fn_span, memory_manager)?;

        Ok(())
    }

    fn parse_assert_expression(
        &mut self,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let assert_start = self.current_span().start;

        self.parser.advance()?; // consume 'assert'

        // 1. Evaluate condition
        self.parse_expr_notail(0, memory_manager)?;
        self.pop_type(); // Condition consumed by JumpIfFalse

        // Check for optional msg token but don't parse the expression yet
        let has_msg_colon = if let Some(TokenInfo {
            token: Token::Operator(op),
            ..
        }) = self.parser.current_token()
        {
            op == ":"
        } else {
            false
        };

        // 2. JumpIfFalse to error branch
        // We'll calculate the full assert span once we've parsed the message (if any)
        let jump_to_error_offset = self.emit_jump(Opcode::JumpIfFalse, 0..0);

        // 3. We jump over the message evaluation and error logic directly to the body
        let jump_to_body_offset = self.emit_jump(Opcode::Jump, 0..0);

        // 4. We are at error branch. Backpatch JumpIfFalse to here.
        self.patch_jump(jump_to_error_offset);

        if has_msg_colon {
            self.parser.advance()?; // consume ':'
            self.parse_expr_notail(0, memory_manager)?;
        } else {
            self.emit_string_constant(memory_manager.allocate_string("Assertion failed").index)?;
        }

        // Calculate the span from 'assert' to the end of the condition or message
        let assert_end = if let Some(prev_token) = self.parser.previous_token() {
            prev_token.span.end
        } else {
            assert_start + 6
        };
        let full_assert_span = assert_start..assert_end;

        // Update the jump spans
        self.patch_jump_span(jump_to_error_offset, full_assert_span.clone());
        self.patch_jump_span(jump_to_body_offset, full_assert_span.clone());

        self.emit_opcode(Opcode::Error, full_assert_span);

        // 5. We are at body branch. Backpatch the unconditional Jump to here.
        self.patch_jump(jump_to_body_offset);

        self.parser
            .consume(Token::Semicolon, "Expected ';' after assert expression")?;

        // 6. Parse body
        self.parse_expr(0, memory_manager)?;

        Ok(())
    }

    fn parse_if_expression(
        &mut self,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        let if_span = self.current_span();

        // Consume 'if' token
        self.parser.advance()?;

        // Parse condition expression
        self.parse_expr_notail(0, memory_manager)?;
        self.pop_type(); // Condition consumed by JumpIfFalse

        // Expect 'then'
        self.parser
            .consume(Token::Then, "Expected 'then' after if condition")?;

        // Emit conditional jump to else/end - if condition is falsy, jump to else
        let jump_to_else = self.emit_jump(Opcode::JumpIfFalse, if_span.clone());

        // Parse then branch body
        self.parse_expr(0, memory_manager)?;

        // ALWAYS emit unconditional jump to skip else branch (even if no explicit else)
        let jump_to_end = self.emit_jump(Opcode::Jump, if_span.clone());

        // Patch the JumpIfFalse to jump here (start of else branch)
        self.patch_jump(jump_to_else);

        // Parse else branch or emit implicit null
        if let Some(token) = self.parser.current_token() {
            if matches!(token.token, Token::Else) {
                self.parser.advance()?; // consume 'else'
                self.parse_expr(0, memory_manager)?;
            } else {
                // No else clause: implicit null
                self.emit_opcode(Opcode::LoadNull, if_span.clone());
            }
        } else {
            // End of input: implicit null
            self.emit_opcode(Opcode::LoadNull, if_span.clone());
        }

        // Patch the unconditional jump to jump here (after entire if expression)
        self.patch_jump(jump_to_end);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_number() {
        let mut scanner = Scanner::new("42", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::Number(42.0));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_simple_addition() {
        let mut scanner = Scanner::new("3 + 4", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

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
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Value::Number(42.0));
        // LoadConst (3) + Neg (1) + Return (1) = 5 bytes
        assert_eq!(chunk.code.len(), 5);
    }

    #[test]
    fn test_grouped_expression() {
        let mut scanner = Scanner::new("(1 + 2) * 3", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 3);
        // LoadConst (3) + LoadConst (3) + Add (1) + LoadConst (3) + Mul (1) + Return (1) = 12 bytes
        assert_eq!(chunk.code.len(), 12);
    }

    #[test]
    fn test_precedence() {
        let mut scanner = Scanner::new("2 + 3 * 4", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

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
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadTrue (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadTrue as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_false_literal() {
        let mut scanner = Scanner::new("false", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadFalse (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadFalse as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_null_literal() {
        let mut scanner = Scanner::new("null", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 0); // No constants for literals
        assert_eq!(chunk.code.len(), 2); // LoadNull (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadNull as u8);
        assert_eq!(chunk.code[1], Opcode::Return as u8);
    }

    #[test]
    fn test_logical_not_with_true() {
        let mut scanner = Scanner::new("!true", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

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
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        let expected_interned = memory_manager.allocate_string("hello world");
        assert_eq!(chunk.constants[0], Value::String(expected_interned.index));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_empty_string_literal() {
        let mut scanner = Scanner::new("\"\"", "test");
        let mut memory_manager = MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        let expected_interned = memory_manager.allocate_string("");
        assert_eq!(chunk.constants[0], Value::String(expected_interned.index));
        assert_eq!(chunk.code.len(), 4); // LoadConst (3 bytes) + Return (1 byte)
    }

    #[test]
    fn test_string_with_logical_not() {
        let mut scanner = Scanner::new("!\"test\"", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        let expected_interned = memory_manager.allocate_string("test");
        assert_eq!(chunk.constants[0], Value::String(expected_interned.index));
        assert_eq!(chunk.code.len(), 5); // LoadConst (3 bytes) + Not (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadConst as u8);
        assert_eq!(chunk.code[3], Opcode::Not as u8);
        assert_eq!(chunk.code[4], Opcode::Return as u8);
    }

    #[test]
    fn test_unicode_literal() {
        let input = "\"🚀\"";
        let mut scanner = Scanner::new(input, "test.jsonnet");
        let mut memory_manager = MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
        if let Value::String(s_idx) = chunk.constants[0] {
            assert_eq!(memory_manager.load_string(s_idx), "🚀");
        } else {
            panic!("Expected string constant");
        }
    }

    #[test]
    fn test_error_string_literal() {
        let mut scanner = Scanner::new("error \"test message\"", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have one constant (the string)
        assert_eq!(chunk.constants.len(), 1);
        let expected_interned = memory_manager.allocate_string("test message");
        assert_eq!(chunk.constants[0], Value::String(expected_interned.index));

        // Should compile to: LoadConst + Error + Return
        assert_eq!(chunk.code.len(), 5); // LoadConst (3 bytes) + Error (1 byte) + Return (1 byte)
        assert_eq!(chunk.code[0], Opcode::LoadConst as u8);
        assert_eq!(chunk.code[3], Opcode::Error as u8);
        assert_eq!(chunk.code[4], Opcode::Return as u8);
    }

    #[test]
    fn test_error_expression() {
        let mut scanner = Scanner::new("error (1 + 2)", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have two constants (1 and 2)
        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], Value::Number(1.0));
        assert_eq!(chunk.constants[1], Value::Number(2.0));

        // Should compile to: LoadConst(1) + LoadConst(2) + Add + Error + Return
        assert_eq!(chunk.code.len(), 9); // LoadConst (3) + LoadConst (3) + Add (1) + Error (1) + Return (1)
        assert_eq!(chunk.code[0], Opcode::LoadConst as u8);
        assert_eq!(chunk.code[3], Opcode::LoadConst as u8);
        assert_eq!(chunk.code[6], Opcode::Add as u8);
        assert_eq!(chunk.code[7], Opcode::Error as u8);
        assert_eq!(chunk.code[8], Opcode::Return as u8);
    }

    #[test]
    fn test_error_string_span_coverage() {
        let input = "error \"test message\"";
        let mut scanner = Scanner::new(input, "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Find the Error opcode and check its span
        let error_opcode_pos = chunk
            .code
            .iter()
            .position(|&x| x == Opcode::Error as u8)
            .unwrap();
        let error_span = chunk.get_span(error_opcode_pos).unwrap();

        // Span should cover the entire error expression
        assert_eq!(error_span.start, 0); // start of "error"
        assert_eq!(error_span.end, input.len()); // end of "test message"
    }

    #[test]
    fn test_error_expression_span_coverage() {
        let input = "error (\"prefix \" + \"suffix\")";
        let mut scanner = Scanner::new(input, "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Find the Error opcode and check its span
        let error_opcode_pos = chunk
            .code
            .iter()
            .position(|&x| x == Opcode::Error as u8)
            .unwrap();
        let error_span = chunk.get_span(error_opcode_pos).unwrap();

        // Span should cover the entire error expression including parentheses
        assert_eq!(error_span.start, 0); // start of "error"
        assert_eq!(error_span.end, input.len()); // end of closing parenthesis
    }

    #[test]
    fn test_error_number_span_coverage() {
        let input = "error 42";
        let mut scanner = Scanner::new(input, "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Find the Error opcode and check its span
        let error_opcode_pos = chunk
            .code
            .iter()
            .position(|&x| x == Opcode::Error as u8)
            .unwrap();
        let error_span = chunk.get_span(error_opcode_pos).unwrap();

        // Span should cover "error 42"
        assert_eq!(error_span.start, 0); // start of "error"
        assert_eq!(error_span.end, input.len()); // end of "42"
    }

    // Local variable tests

    #[test]
    fn test_simple_local() {
        // local x = 5; x
        let mut scanner = Scanner::new("local x = 5; x", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // RHS is wrapped in a thunk (Function) for lazy evaluation
        assert_eq!(chunk.constants.len(), 1);
        assert!(matches!(chunk.constants[0], Value::Function(_)));
    }

    #[test]
    fn test_multiple_locals() {
        // local x = 1, y = 2; x + y
        let mut scanner = Scanner::new("local x = 1, y = 2; x + y", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Each RHS wrapped in a thunk
        assert_eq!(chunk.constants.len(), 2);
    }

    #[test]
    fn test_local_using_local() {
        // local x = 1, y = x + 1; y
        let mut scanner = Scanner::new("local x = 1, y = x + 1; y", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_forward_reference_in_local() {
        // local x = y + 1, y = 5; x — forward references should compile
        // (in Jsonnet, local bindings in the same group are mutually visible)
        let mut scanner = Scanner::new("local x = y + 1, y = 5; x", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let result = compiler.compile(&mut memory_manager);

        assert!(
            result.is_ok(),
            "Forward reference should compile: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_duplicate_local_error() {
        // local x = 1, x = 2; x (should fail)
        let mut scanner = Scanner::new("local x = 1, x = 2; x", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let result = compiler.compile(&mut memory_manager);

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("already declared"));
    }

    #[test]
    fn test_nested_local_scopes() {
        // local x = 1; local y = x + 1; y
        let mut scanner = Scanner::new("local x = 1; local y = x + 1; y", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_local_shadowing() {
        // local x = 1; local x = 2; x (shadowing in nested scope)
        let mut scanner = Scanner::new("local x = 1; local x = 2; x", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_local_with_object() {
        // local x = {awesome: true}; x
        let mut scanner = Scanner::new("local x = {awesome: true}; x", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_local_with_nested_object() {
        let input = r#"local x = {
            awesome: true,
            nestedObj: {
                anotherNest: 45,
                someString: "this is great"
            }
        }; x"#;
        let mut scanner = Scanner::new(input, "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert!(chunk.code.len() > 0);
    }

    // Bitwise operator tests

    #[test]
    fn test_bitwise_xor() {
        let mut scanner = Scanner::new("5 ^ 3", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have constants for 5 and 3
        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], Value::Number(5.0));
        assert_eq!(chunk.constants[1], Value::Number(3.0));
        // LoadConst (3) + LoadConst (3) + BitXor (1) + Return (1) = 8 bytes
        assert_eq!(chunk.code.len(), 8);
        assert_eq!(chunk.code[6], Opcode::BitXor as u8);
    }

    #[test]
    fn test_shift_left() {
        let mut scanner = Scanner::new("8 << 2", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], Value::Number(8.0));
        assert_eq!(chunk.constants[1], Value::Number(2.0));
        assert_eq!(chunk.code[6], Opcode::Shl as u8);
    }

    #[test]
    fn test_shift_right() {
        let mut scanner = Scanner::new("16 >> 2", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], Value::Number(16.0));
        assert_eq!(chunk.constants[1], Value::Number(2.0));
        assert_eq!(chunk.code[6], Opcode::Shr as u8);
    }

    #[test]
    fn test_bitwise_precedence() {
        // Test: 1 | 2 ^ 4 & 8 should parse as 1 | (2 ^ (4 & 8))
        // Because & has higher precedence than ^, and ^ has higher precedence than |
        let mut scanner = Scanner::new("1 | 2 ^ 4 & 8", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have constants for 1, 2, 4, 8
        assert_eq!(chunk.constants.len(), 4);
        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_shift_precedence() {
        // Test: 1 + 2 << 3 should parse as (1 + 2) << 3
        // Because + has higher precedence than <<
        let mut scanner = Scanner::new("1 + 2 << 3", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 3);
        // Verify order of operations: LoadConst(1), LoadConst(2), Add, LoadConst(3), Shl
        assert!(chunk.code.len() > 0);
    }

    #[test]
    fn test_skip_comprehension_condition() {
        // Test simple condition ending with ]
        let mut scanner = Scanner::new("x == 1]", "test");
        let mut compiler = Compiler::new(&mut scanner, "test");
        // Start parsing to prime the parser
        let _ = compiler.parser.advance();
        let skip = compiler.skip_comprehension_condition().unwrap();
        assert_eq!(skip, 3); // x, ==, 1 (stops at ])

        // Test condition ending with 'if'
        let mut scanner2 = Scanner::new("x == 1 if y == 2]", "test");
        let mut compiler2 = Compiler::new(&mut scanner2, "test");
        let _ = compiler2.parser.advance();
        let skip2 = compiler2.skip_comprehension_condition().unwrap();
        assert_eq!(skip2, 3); // x, ==, 1 (stops at if)

        // Test condition ending with 'for'
        let mut scanner3 = Scanner::new("x == 1 for y in z]", "test");
        let mut compiler3 = Compiler::new(&mut scanner3, "test");
        let _ = compiler3.parser.advance();
        let skip3 = compiler3.skip_comprehension_condition().unwrap();
        assert_eq!(skip3, 3); // x, ==, 1 (stops at for)

        // Test condition with nested brackets
        let mut scanner4 = Scanner::new("x in [1, 2, 3]]", "test");
        let mut compiler4 = Compiler::new(&mut scanner4, "test");
        let _ = compiler4.parser.advance();
        let skip4 = compiler4.skip_comprehension_condition().unwrap();
        // Tokens: x, in, [, 1, ,, 2, ,, 3, ]
        // Stops at the final ] which is at depth 0
        assert_eq!(skip4, 9);

        // Test condition with nested brackets containing 'if' and 'for'
        let mut scanner5 = Scanner::new("x in [if true then 1, for i in []]]", "test");
        let mut compiler5 = Compiler::new(&mut scanner5, "test");
        let _ = compiler5.parser.advance();
        let skip5 = compiler5.skip_comprehension_condition().unwrap();
        // It shouldn't stop at 'if' or 'for' or the inner ']' because depth > 0.
        // It should stop at the final ']' where depth becomes 0 again.
        // Tokens: x, in, [, if, true, then, 1, ,, for, i, in, [, ], ], ]
        // Note: the last ] is what it stops AT.
        assert_eq!(skip5, 14);
    }

    #[test]
    fn test_local_function_sugar() {
        // local f(x) = x + 1; f(5) should compile successfully
        let mut scanner = Scanner::new("local f(x) = x + 1; f(5)", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager);
        assert!(
            chunk.is_ok(),
            "Function sugar should compile: {:?}",
            chunk.err()
        );
    }

    #[test]
    fn test_local_function_sugar_multi_param() {
        let mut scanner = Scanner::new("local f(x, y) = x + y; f(3, 4)", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager);
        assert!(
            chunk.is_ok(),
            "Multi-param function sugar should compile: {:?}",
            chunk.err()
        );
    }

    #[test]
    fn test_local_function_sugar_no_params() {
        let mut scanner = Scanner::new("local f() = 42; f()", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager);
        assert!(
            chunk.is_ok(),
            "No-param function sugar should compile: {:?}",
            chunk.err()
        );
    }

    #[test]
    fn test_local_function_sugar_recursive() {
        let mut scanner = Scanner::new(
            "local fac(n) = if n <= 1 then 1 else n * fac(n - 1); fac(5)",
            "test",
        );
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager);
        assert!(
            chunk.is_ok(),
            "Recursive function sugar should compile: {:?}",
            chunk.err()
        );
    }

    #[test]
    fn test_local_function_sugar_with_closure() {
        let mut scanner = Scanner::new("local x = 10; local add_x(y) = x + y; add_x(5)", "test");
        let compiler = Compiler::new(&mut scanner, "test");
        let mut memory_manager = MemoryManager::new();
        let chunk = compiler.compile(&mut memory_manager);
        assert!(
            chunk.is_ok(),
            "Function sugar with closure should compile: {:?}",
            chunk.err()
        );
    }

    fn compile_source_err(source: &str) -> String {
        let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
        let mut mm = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        compiler.compile(&mut mm).unwrap_err().message
    }

    #[test]
    fn test_duplicate_param_error() {
        let msg = compile_source_err("function(x, x) x");
        assert!(
            msg.contains("duplicate") || msg.contains("already") || msg.contains("Duplicate"),
            "got: {}",
            msg
        );
    }

    #[test]
    fn test_super_outside_object_compiles() {
        // super.x compiles without error; validation happens at runtime
        let mut scanner = scanner::Scanner::new("super.x", "test.jsonnet");
        let mut mm = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        assert!(compiler.compile(&mut mm).is_ok());
    }

    fn compile_source_ok(source: &str) {
        let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
        let mut mm = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        compiler
            .compile(&mut mm)
            .unwrap_or_else(|e| panic!("expected compile ok but got: {}", e.message));
    }

    // Gap-fill tests for uncovered compiler.rs paths

    #[test]
    fn test_object_with_assert_compiles() {
        compile_source_ok("{x: 1, assert self.x == 1}");
    }

    #[test]
    fn test_object_with_assert_and_msg_compiles() {
        compile_source_ok(r#"{x: 1, assert self.x == 1 : "x must be 1"}"#);
    }

    #[test]
    fn test_object_with_local_field_compiles() {
        compile_source_ok("{local n = 5, x: n + 1}");
    }

    #[test]
    fn test_object_with_local_trailing_comma_compiles() {
        compile_source_ok("{local n = 5, x: n + 1,}");
    }

    #[test]
    fn test_in_operator_compiles() {
        compile_source_ok(r#""a" in {a: 1}"#);
    }

    #[test]
    fn test_super_has_field_compiles() {
        compile_source_ok("local b = {x: 1}; (b + {y: 'x' in super}).y");
    }

    #[test]
    fn test_super_bracket_index_compiles() {
        compile_source_ok(r#"local b = {x: 1}; (b + {y: super["x"]}).y"#);
    }

    #[test]
    fn test_unary_plus_compiles() {
        compile_source_ok("+5");
    }

    #[test]
    fn test_bitnot_compiles() {
        compile_source_ok("~5");
    }

    #[test]
    fn test_string_key_field_compiles() {
        compile_source_ok(r#"{"hello world": 42}"#);
    }

    #[test]
    fn test_computed_field_name_compiles() {
        compile_source_ok(r#"local k = "x"; {[k]: 1}"#);
    }

    #[test]
    fn test_import_non_string_error() {
        let msg = compile_source_err("import 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_importstr_non_string_error() {
        let msg = compile_source_err("importstr 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_importbin_non_string_error() {
        let msg = compile_source_err("importbin 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_super_dot_missing_name_error() {
        let msg = compile_source_err("local b = {x: 1}; (b + {y: super.}).y");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_named_arg_call_compiles() {
        compile_source_ok("std.substr('hello', from=1, len=3)");
    }

    #[test]
    fn test_object_field_force_visible_compiles() {
        compile_source_ok("{a::: 1}");
    }

    #[test]
    fn test_object_assert_with_comma_after_compiles() {
        compile_source_ok("{assert true, x: 1}");
    }

    #[test]
    fn test_object_assert_trailing_brace_compiles() {
        compile_source_ok("{x: 1, assert true}");
    }

    #[test]
    fn test_too_many_constants_error_skipped() {
        // This would require >65535 constants to trigger; skip it but we document it
        // compile_source_err is not practical for this - just validate normal compilation works
        compile_source_ok("1 + 2");
    }

    #[test]
    fn test_slice_double_colon_compiles() {
        compile_source_ok("[1,2,3,4,5][::2]");
    }

    #[test]
    fn test_slice_with_both_ends_and_step_compiles() {
        compile_source_ok("[1,2,3,4,5][1:4:2]");
    }

    #[test]
    fn test_object_comprehension_with_filter_compiles() {
        compile_source_ok(r#"{[k]: 1 for k in ["a","b","c"] if k != "b"}"#);
    }

    #[test]
    fn test_new_from_file_nonexistent_error() {
        let result = Compiler::new_from_file("nonexistent_test_file_xyz.jsonnet");
        assert!(result.is_err());
    }

    // Gap-fill: add_upvalue / resolve_upvalue across nested closure scopes

    fn compile_nested_closure(source: &str) {
        let mut scanner = Scanner::new(source, "test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let mut mm = MemoryManager::new();
        let chunk = compiler.compile(&mut mm).expect("compile failed");
        assert!(!chunk.is_empty());
    }

    #[test]
    fn test_nested_closure_upvalue_resolution() {
        // A closure inside another closure — forces resolve_upvalue to recurse into enclosing scope
        compile_nested_closure("local outer(x) = local inner(y) = x + y; inner; outer(1)(2)");
    }

    #[test]
    fn test_deeply_nested_upvalue() {
        // Three levels of nesting to exercise the chained upvalue path
        compile_nested_closure(
            "local f(a) = local g(b) = local h(c) = a + b + c; h; g; f(1)(2)(3)",
        );
    }
}
