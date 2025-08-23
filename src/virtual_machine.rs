use std::ops::Range;
use chunk::{Chunk, Opcode};
use scanner::ScanError;

/// Expanded value type for the virtual machine
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
}

impl Value {
    /// Check if value is truthy according to Jsonnet rules
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0,
        }
    }

    /// Convert value to f64 for numeric operations
    pub fn to_number<'a>(&self, span: Range<usize>, source_id: &'a str) -> Result<f64, RuntimeError> {
        match self {
            Value::Number(n) => Ok(*n),
            _ => Err(RuntimeError {
                span,
                message: format!("Cannot convert {:?} to number", self),
                source_id: source_id.to_string(),
            }),
        }
    }

    /// Convert to integer for bitwise operations (per Jsonnet spec)
    pub fn to_integer<'a>(&self, span: Range<usize>, source_id: &'a str) -> Result<i64, RuntimeError> {
        match self {
            Value::Number(n) => {
                if n.is_nan() || n.is_infinite() {
                    Err(RuntimeError {
                        span,
                        message: "Cannot convert NaN or Infinity to integer".to_string(),
                        source_id: source_id.to_string(),
                    })
                } else {
                    Ok(*n as i64)
                }
            }
            _ => Err(RuntimeError {
                span,
                message: format!("Cannot convert {:?} to integer", self),
                source_id: source_id.to_string(),
            }),
        }
    }
}

/// Runtime error type - alias for ScanError to reuse existing infrastructure
pub type RuntimeError = ScanError;

/// Virtual machine for executing Jsonnet bytecode
pub struct VirtualMachine<'a> {
    /// Collection of chunks that can be executed
    chunks: Vec<Chunk<'a>>,
    /// Index of the currently executing chunk
    current_chunk: usize,
    /// Program counter within the current chunk
    program_counter: usize,
    /// Execution stack
    stack: Vec<Value>,
}

impl<'a> VirtualMachine<'a> {
    /// Create a new virtual machine with the given starting chunk
    pub fn new(chunk: Chunk<'a>) -> Self {
        let mut vm = Self {
            chunks: Vec::new(),
            current_chunk: 0,
            program_counter: 0,
            stack: Vec::with_capacity(1024),
        };

        vm.chunks.push(chunk);
        vm
    }

    /// Get the current chunk being executed
    fn current_chunk(&self) -> &Chunk<'a> {
        &self.chunks[self.current_chunk]
    }

    /// Push a value onto the stack, checking for overflow
    fn push(&mut self, value: Value) -> Result<(), RuntimeError> {
        const MAX_STACK_SIZE: usize = 65536;

        if self.stack.len() >= MAX_STACK_SIZE {
            return Err(RuntimeError {
                span: self.get_current_span(),
                message: "Stack overflow - maximum stack size exceeded".to_string(),
                source_id: self.current_chunk().source_id.to_string(),
            });
        }

        // Grow stack capacity if needed
        if self.stack.len() == self.stack.capacity() && self.stack.capacity() < MAX_STACK_SIZE {
            let new_capacity = (self.stack.capacity() * 2).min(MAX_STACK_SIZE);
            self.stack.reserve(new_capacity - self.stack.capacity());
        }

        self.stack.push(value);
        Ok(())
    }

    /// Pop a value from the stack, checking for underflow
    fn pop(&mut self) -> Result<Value, RuntimeError> {
        self.stack.pop().ok_or_else(|| RuntimeError {
            span: self.get_current_span(),
            message: "Stack underflow - attempted to pop from empty stack".to_string(),
            source_id: self.current_chunk().source_id.to_string(),
        })
    }

    /// Peek at the top value without popping
    fn peek(&self) -> Result<&Value, RuntimeError> {
        self.stack.last().ok_or_else(|| RuntimeError {
            span: self.get_current_span(),
            message: "Stack underflow - attempted to peek empty stack".to_string(),
            source_id: self.current_chunk().source_id.to_string(),
        })
    }

    /// Get the source span for the current instruction
    fn get_current_span(&self) -> Range<usize> {
        self.current_chunk()
            .get_span(self.program_counter)
            .cloned()
            .unwrap_or(0..0)
    }

    /// Read a u16 operand from the current position and advance PC
    fn read_u16_operand(&mut self) -> Result<u16, RuntimeError> {
        let chunk = self.current_chunk();
        let operand = chunk.read_u16(self.program_counter + 1).ok_or_else(|| RuntimeError {
            span: self.get_current_span(),
            message: "Invalid bytecode - missing operand".to_string(),
            source_id: chunk.source_id.to_string(),
        })?;

        self.program_counter += 3; // opcode + 2 bytes for u16
        Ok(operand)
    }

    /// Advance program counter by 1 (for opcodes with no operands)
    fn advance_pc(&mut self) {
        self.program_counter += 1;
    }

    /// Main interpretation loop
    pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
        loop {
            let chunk = self.current_chunk();

            // Check if we've reached the end
            if self.program_counter >= chunk.count() {
                return Err(RuntimeError {
                    span: self.get_current_span(),
                    message: "Unexpected end of bytecode - missing Return instruction".to_string(),
                    source_id: chunk.source_id.to_string(),
                });
            }

            let opcode = chunk.read_opcode(self.program_counter).ok_or_else(|| RuntimeError {
                span: self.get_current_span(),
                message: "Invalid opcode in bytecode".to_string(),
                source_id: chunk.source_id.to_string(),
            })?;

            match opcode {
                Opcode::LoadNull => {
                    self.push(Value::Null)?;
                    self.advance_pc();
                }

                Opcode::LoadTrue => {
                    self.push(Value::Boolean(true))?;
                    self.advance_pc();
                }

                Opcode::LoadFalse => {
                    self.push(Value::Boolean(false))?;
                    self.advance_pc();
                }

                Opcode::LoadConst => {
                    let index = self.read_u16_operand()?;
                    let chunk = self.current_chunk();

                    if index as usize >= chunk.constants.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Invalid constant index: {}", index),
                            source_id: chunk.source_id.to_string(),
                        });
                    }

                    let constant = chunk.constants[index as usize];
                    self.push(Value::Number(constant))?;
                }

                // Binary arithmetic operations
                Opcode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? + b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? - b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? * b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let b_num = b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    if b_num == 0.0 {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Division by zero".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? / b_num;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Comparison operations
                Opcode::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? < b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Le => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? <= b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? > b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ge => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? >= b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                // Bitwise operations
                Opcode::Shl => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let shift_count = (b.to_integer(self.get_current_span(), self.current_chunk().source_id)? % 64) as u32;
                    if shift_count >= 64 {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Invalid shift count".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    let result = (a.to_integer(self.get_current_span(), self.current_chunk().source_id)? << shift_count) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Shr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let shift_count = (b.to_integer(self.get_current_span(), self.current_chunk().source_id)? % 64) as u32;
                    let result = (a.to_integer(self.get_current_span(), self.current_chunk().source_id)? >> shift_count) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitAnd => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (a.to_integer(self.get_current_span(), self.current_chunk().source_id)? & b.to_integer(self.get_current_span(), self.current_chunk().source_id)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitXor => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (a.to_integer(self.get_current_span(), self.current_chunk().source_id)? ^ b.to_integer(self.get_current_span(), self.current_chunk().source_id)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitOr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (a.to_integer(self.get_current_span(), self.current_chunk().source_id)? | b.to_integer(self.get_current_span(), self.current_chunk().source_id)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Logical operations
                Opcode::LogicalAnd => {
                    let a = self.pop()?;
                    if !a.is_truthy() {
                        self.push(Value::Boolean(false))?;
                    } else {
                        let b = self.pop()?;
                        self.push(Value::Boolean(b.is_truthy()))?;
                    }
                    self.advance_pc();
                }

                Opcode::LogicalOr => {
                    let a = self.pop()?;
                    if a.is_truthy() {
                        self.push(Value::Boolean(true))?;
                    } else {
                        let b = self.pop()?;
                        self.push(Value::Boolean(b.is_truthy()))?;
                    }
                    self.advance_pc();
                }

                // Unary operations
                Opcode::Neg => {
                    let a = self.pop()?;
                    let result = -a.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Pos => {
                    let a = self.pop()?;
                    let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)?; // Unary + is essentially a no-op for numbers
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Not => {
                    let a = self.pop()?;
                    let result = !a.is_truthy();
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::BitNot => {
                    let a = self.pop()?;
                    let result = (!a.to_integer(self.get_current_span(), self.current_chunk().source_id)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Stack operations
                Opcode::Pop => {
                    self.pop()?;
                    self.advance_pc();
                }

                Opcode::Dup => {
                    let value = self.peek()?.clone();
                    self.push(value)?;
                    self.advance_pc();
                }

                Opcode::Swap => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(a)?;
                    self.advance_pc();
                }

                Opcode::Return => {
                    // Return the top value and halt execution
                    return self.pop();
                }

                // All other opcodes result in runtime error
                _ => {
                    return Err(RuntimeError {
                        span: self.get_current_span(),
                        message: format!("Unimplemented opcode: {:?}", opcode),
                        source_id: self.current_chunk().source_id.to_string(),
                    });
                }
            }
        }
    }
}

/// Main execution function - entry point for running Jsonnet bytecode
pub fn execute(chunk: Chunk) -> Result<serde_json::Value, RuntimeError> {
    let mut vm = VirtualMachine::new(chunk);

    match vm.interpret() {
        Ok(value) => {
            // Convert VM value to JSON value
            let json_value = match value {
                Value::Null => serde_json::Value::Null,
                Value::Boolean(b) => serde_json::Value::Bool(b),
                Value::Number(n) => serde_json::Value::Number(
                    serde_json::Number::from_f64(n)
                        .ok_or_else(|| RuntimeError {
                            span: vm.get_current_span(),
                            message: "Invalid number".to_string(),
                            source_id: vm.current_chunk().source_id.to_string(),
                        })?
                ),
            };
            Ok(json_value)
        }
        Err(error) => {
            // Print error report using ariadne
            let _report = error.into_report();
            // TODO: Need source content for proper error display
            eprintln!("Runtime Error: {}", error.message);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chunk::{Chunk, Opcode};

    /// Helper function to create a test chunk
    fn create_test_chunk() -> Chunk<'static> {
        Chunk::new("test.jsonnet")
    }

    #[test]
    fn test_value_truthiness() {
        assert!(!Value::Null.is_truthy());
        assert!(!Value::Boolean(false).is_truthy());
        assert!(Value::Boolean(true).is_truthy());
        assert!(!Value::Number(0.0).is_truthy());
        assert!(Value::Number(1.0).is_truthy());
        assert!(Value::Number(-1.0).is_truthy());
        assert!(Value::Number(0.1).is_truthy());
    }

    #[test]
    fn test_load_null() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_load_true() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_load_false() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadFalse, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_load_const() {
        let mut chunk = create_test_chunk();
        let const_index = chunk.add_constant(42.0);
        chunk.write_opcode_u16(Opcode::LoadConst, const_index as u16, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_add() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(10.0);
        let idx2 = chunk.add_constant(5.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(15.0));
    }

    #[test]
    fn test_subtract() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(10.0);
        let idx2 = chunk.add_constant(3.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Sub, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_multiply() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(6.0);
        let idx2 = chunk.add_constant(7.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Mul, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_divide() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(15.0);
        let idx2 = chunk.add_constant(3.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Div, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_divide_by_zero() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(10.0);
        let idx2 = chunk.add_constant(0.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Div, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Division by zero"));
    }

    #[test]
    fn test_comparison_lt() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(5.0);
        let idx2 = chunk.add_constant(10.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Lt, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_bitwise_shl() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(8.0);
        let idx2 = chunk.add_constant(2.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Shl, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(32.0)); // 8 << 2 = 32
    }

    #[test]
    fn test_logical_and() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::LoadFalse, 5..10);
        chunk.write_opcode(Opcode::LogicalAnd, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_logical_or() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::LoadFalse, 5..10);
        chunk.write_opcode(Opcode::LogicalOr, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_neg() {
        let mut chunk = create_test_chunk();
        let idx = chunk.add_constant(42.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx as u16, 0..5);
        chunk.write_opcode(Opcode::Neg, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-42.0));
    }

    #[test]
    fn test_not() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Not, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_pop() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(1.0);
        let idx2 = chunk.add_constant(2.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Pop, 10..15); // Pop 2.0
        chunk.write_opcode(Opcode::Return, 15..20); // Return 1.0

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_dup() {
        let mut chunk = create_test_chunk();
        let idx = chunk.add_constant(42.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx as u16, 0..5);
        chunk.write_opcode(Opcode::Dup, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15); // 42 + 42
        chunk.write_opcode(Opcode::Return, 15..20);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(84.0));
    }

    #[test]
    fn test_swap() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(10.0);
        let idx2 = chunk.add_constant(3.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Swap, 10..15); // Now stack is [3, 10]
        chunk.write_opcode(Opcode::Sub, 15..20); // 3 - 10 = -7
        chunk.write_opcode(Opcode::Return, 20..25);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-7.0));
    }

    #[test]
    fn test_stack_underflow() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::Add, 0..5); // Try to add with empty stack
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Stack underflow"));
    }

    #[test]
    fn test_invalid_constant_index() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode_u16(Opcode::LoadConst, 999, 0..5); // Invalid index
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Invalid constant index"));
    }

    #[test]
    fn test_missing_return() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        // Missing Return opcode

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("missing Return instruction"));
    }

    #[test]
    fn test_unimplemented_opcode() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::CreateArray, 0..5); // Unimplemented opcode
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Unimplemented opcode"));
    }

    #[test]
    fn test_complex_expression() {
        // Test: (10 + 5) * 2 - 3 = 27
        let mut chunk = create_test_chunk();
        let idx_10 = chunk.add_constant(10.0);
        let idx_5 = chunk.add_constant(5.0);
        let idx_2 = chunk.add_constant(2.0);
        let idx_3 = chunk.add_constant(3.0);

        chunk.write_opcode_u16(Opcode::LoadConst, idx_10 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx_5 as u16, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15);  // 15
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 15..20);
        chunk.write_opcode(Opcode::Mul, 20..25);  // 30
        chunk.write_opcode_u16(Opcode::LoadConst, idx_3 as u16, 25..30);
        chunk.write_opcode(Opcode::Sub, 30..35);  // 27
        chunk.write_opcode(Opcode::Return, 35..40);

        let mut vm = VirtualMachine::new(chunk);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(27.0));
    }
}
