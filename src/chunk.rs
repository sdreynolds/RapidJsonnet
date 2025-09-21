use std::ops::Range;
use ariadne::{Report, ReportKind, Label};
use scanner::ScanError;
use slotmap::DefaultKey;

/// Runtime error type - alias for ScanError to reuse existing infrastructure
pub type RuntimeError = ScanError;

pub type ObjectIndex = DefaultKey;
pub type StringIndex = DefaultKey;

/// Value type for the Jsonnet virtual machine
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(StringIndex),
    Object(ObjectIndex),
}

impl Value {
    /// Check if value is truthy according to Jsonnet rules
    // @TODO: remove this and make is_truthy part of virtual machine
    // The virutal machine has access to the memory manager so  can actually do
    // truthy ness
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0,
            Value::String(_s) => true,
            Value::Object(_key) => true, // Objects are always truthy
        }
    }

    /// Convert value to f64 for numeric operations
    // @TODO: these need to move virtual machine because of memory manager
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
    // @TODO: these need to move virtual machine because of memory manager
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
            // @TODO: turn string into integer
            _ => Err(RuntimeError {
                span,
                message: format!("Cannot convert {:?} to integer", self),
                source_id: source_id.to_string(),
            }),
        }
    }
}

// Manual implementation of Eq for Value
impl Eq for Value {}

// Manual implementation of Hash for Value
impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Value::Null => {
                0u8.hash(state);
            }
            Value::Boolean(b) => {
                1u8.hash(state);
                b.hash(state);
            }
            Value::Number(n) => {
                2u8.hash(state);
                // For f64, we need to handle the hash carefully
                // We'll use the byte representation, but handle special cases
                if n.is_nan() {
                    // All NaN values hash the same
                    f64::NAN.to_bits().hash(state);
                } else {
                    n.to_bits().hash(state);
                }
            }
            Value::String(s) => {
                3u8.hash(state);
                s.hash(state);
            }
            Value::Object(key) => {
                4u8.hash(state);
                key.hash(state);
            }
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Boolean(b) => write!(f, "{}", b),
            Value::Number(n) => {
                if n.is_nan() {
                    write!(f, "NaN")
                } else if n.is_infinite() {
                    if n.is_sign_positive() {
                        write!(f, "Infinity")
                    } else {
                        write!(f, "-Infinity")
                    }
                } else {
                    write!(f, "{}", n)
                }
            },
            Value::String(index) => write!(f, "String[{:?}]", index),
            Value::Object(index) => write!(f, "Object[{:?}]", index),
        }
    }
}

/// Opcodes for the Jsonnet virtual machine
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opcode {
    // Core Value Operations
    LoadNull = 0,
    LoadTrue = 1,
    LoadFalse = 2,
    LoadConst = 3,    // operand: u16 index
    LoadSelf = 4,
    LoadSuper = 5,
    LoadVar = 6,      // operand: u16 name_index

    // Object Operations
    CreateObject = 10,      // operand: u16 field_count
    CreateObjectComp = 11,
    FieldDef = 12,          // operands: u16 name_index, u8 hidden_type
    Assert = 13,
    ObjectIndex = 14,
    ObjectMerge = 15,

    // Array Operations
    CreateArray = 20,       // operand: u16 element_count
    ArrayIndex = 21,
    ArrayConcat = 22,

    // Function Operations
    CreateFunction = 30,    // operands: u8 param_count, u32 code_offset
    Call = 31,              // operands: u8 positional_count, u8 named_count
    Return = 32,
    BindDefault = 33,       // operand: u16 param_name

    // Control Flow
    Jump = 40,              // operand: i32 offset
    JumpIfFalse = 41,       // operand: i32 offset
    JumpIfTrue = 42,        // operand: i32 offset
    LocalScope = 43,        // operand: u8 var_count

    // Binary Operators
    Add = 50,
    Sub = 51,
    Mul = 52,
    Div = 53,
    Lt = 54,
    Le = 55,
    Gt = 56,
    Ge = 57,
    Eq = 58,
    Ne = 59,
    Shl = 60,
    Shr = 61,
    BitAnd = 62,
    BitXor = 63,
    BitOr = 64,
    LogicalAnd = 65,
    LogicalOr = 66,
    StringConcat = 67,

    // Unary Operators
    Neg = 70,
    Pos = 71,
    Not = 72,
    BitNot = 73,

    // Standard Library Integration
    StdCall = 80,           // operands: u16 function_index, u8 arg_count
    Error = 81,

    // Stack Management
    Pop = 90,
    Dup = 91,
    Swap = 92,
}

impl Opcode {
    /// Convert a u8 to an Opcode, returning None if invalid
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Opcode::LoadNull),
            1 => Some(Opcode::LoadTrue),
            2 => Some(Opcode::LoadFalse),
            3 => Some(Opcode::LoadConst),
            4 => Some(Opcode::LoadSelf),
            5 => Some(Opcode::LoadSuper),
            6 => Some(Opcode::LoadVar),
            10 => Some(Opcode::CreateObject),
            11 => Some(Opcode::CreateObjectComp),
            12 => Some(Opcode::FieldDef),
            13 => Some(Opcode::Assert),
            14 => Some(Opcode::ObjectIndex),
            15 => Some(Opcode::ObjectMerge),
            20 => Some(Opcode::CreateArray),
            21 => Some(Opcode::ArrayIndex),
            22 => Some(Opcode::ArrayConcat),
            30 => Some(Opcode::CreateFunction),
            31 => Some(Opcode::Call),
            32 => Some(Opcode::Return),
            33 => Some(Opcode::BindDefault),
            40 => Some(Opcode::Jump),
            41 => Some(Opcode::JumpIfFalse),
            42 => Some(Opcode::JumpIfTrue),
            43 => Some(Opcode::LocalScope),
            50 => Some(Opcode::Add),
            51 => Some(Opcode::Sub),
            52 => Some(Opcode::Mul),
            53 => Some(Opcode::Div),
            54 => Some(Opcode::Lt),
            55 => Some(Opcode::Le),
            56 => Some(Opcode::Gt),
            57 => Some(Opcode::Ge),
            58 => Some(Opcode::Eq),
            59 => Some(Opcode::Ne),
            60 => Some(Opcode::Shl),
            61 => Some(Opcode::Shr),
            62 => Some(Opcode::BitAnd),
            63 => Some(Opcode::BitXor),
            64 => Some(Opcode::BitOr),
            65 => Some(Opcode::LogicalAnd),
            66 => Some(Opcode::LogicalOr),
            67 => Some(Opcode::StringConcat),
            70 => Some(Opcode::Neg),
            71 => Some(Opcode::Pos),
            72 => Some(Opcode::Not),
            73 => Some(Opcode::BitNot),
            80 => Some(Opcode::StdCall),
            81 => Some(Opcode::Error),
            90 => Some(Opcode::Pop),
            91 => Some(Opcode::Dup),
            92 => Some(Opcode::Swap),
            _ => None,
        }
    }
}

/// Hidden field types for object fields
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldVisibility {
    Visible = 0,      // :
    Hidden = 1,       // ::
    ForceVisible = 2, // :::
}

/// Represents a run-length encoding for spans
/// This struct maps code indices to their corresponding source code spans
/// in an efficient way by storing only unique spans and their repetition counts
#[derive(Debug, Clone, PartialEq)]
pub struct SpanRunLength {
    /// The span in the source code
    pub span: Range<usize>,
    /// The count of opcodes/operands that share the same span
    pub repeated_values: usize,
}

impl SpanRunLength {
    /// Creates a new SpanRunLength entry
    pub fn new(span: Range<usize>, repeated_values: usize) -> Self {
        Self {
            span,
            repeated_values,
        }
    }
}

/// A chunk represents a collection of bytecode instructions and associated metadata
/// for the virtual machine to execute
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk<'a> {
    /// Source identifier used with ariadne library
    pub source_id: &'a str,
    /// Vector of bytecode containing opcodes and operands
    pub code: Vec<u8>,
    /// Vector mapping code indices to spans using run-length encoding
    pub spans: Vec<SpanRunLength>,
    /// Vector of constant values referenced by the bytecode
    pub constants: Vec<Value>,
}

impl<'a> Chunk<'a> {
    /// Creates a new empty chunk with the given source identifier
    pub fn new(source_id: &'a str) -> Self {
        Self {
            source_id,
            code: Vec::new(),
            spans: Vec::new(),
            constants: Vec::new(),
        }
    }

    /// Writes a byte to the chunk's code with the associated source span
    fn write(&mut self, byte: u8, span: Range<usize>) {
        self.code.push(byte);

        // Update span information using run-length encoding
        if let Some(last_span) = self.spans.last_mut() {
            if last_span.span == span {
                // Same span as previous instruction, increment count
                last_span.repeated_values += 1;
            } else {
                // New span, create new entry
                self.spans.push(SpanRunLength::new(span, 1));
            }
        } else {
            // First instruction
            self.spans.push(SpanRunLength::new(span, 1));
        }
    }

    /// Adds a constant value to the chunk and returns its index
    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    /// Gets the span for a given instruction index
    pub fn get_span(&self, instruction_index: usize) -> Option<&Range<usize>> {
        let mut current_index = 0;

        for span_info in &self.spans {
            if instruction_index < current_index + span_info.repeated_values {
                return Some(&span_info.span);
            }
            current_index += span_info.repeated_values;
        }

        None
    }

    /// Creates an ariadne error report for a range of code offsets with the given message
    pub fn create_error_report(&self, code_range: Range<usize>, message: &str) -> Report<(&str, Range<usize>)> {
        // Find the source spans that correspond to the code range
        let start_span = self.get_span(code_range.start);
        let end_span = self.get_span(code_range.end.saturating_sub(1));

        // Determine the overall source span to highlight
        let source_span = match (start_span, end_span) {
            (Some(start), Some(end)) => start.start..end.end,
            (Some(start), None) => start.clone(),
            (None, Some(end)) => end.clone(),
            (None, None) => 0..0, // Fallback if no spans found
        };

        Report::build(ReportKind::Error, (self.source_id, source_span.clone()))
            .with_message(message)
            .with_label(
                Label::new((self.source_id, source_span))
                    .with_message("error occurred here")
            )
            .finish()
    }

    /// Returns the number of instructions in the chunk
    pub fn count(&self) -> usize {
        self.code.len()
    }

    /// Returns whether the chunk is empty
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// Write an opcode with no operands
    pub fn write_opcode(&mut self, opcode: Opcode, span: Range<usize>) {
        self.write(opcode as u8, span);
    }

    /// Write an opcode with a u8 operand
    pub fn write_opcode_u8(&mut self, opcode: Opcode, operand: u8, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        self.write(operand, span);
    }

    /// Write an opcode with a u16 operand (little-endian)
    pub fn write_opcode_u16(&mut self, opcode: Opcode, operand: u16, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        let bytes = operand.to_le_bytes();
        self.write(bytes[0], span.clone());
        self.write(bytes[1], span);
    }

    /// Write an opcode with a u32 operand (little-endian)
    pub fn write_opcode_u32(&mut self, opcode: Opcode, operand: u32, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        let bytes = operand.to_le_bytes();
        for byte in bytes {
            self.write(byte, span.clone());
        }
    }

    /// Write an opcode with an i32 operand (little-endian)
    pub fn write_opcode_i32(&mut self, opcode: Opcode, operand: i32, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        let bytes = operand.to_le_bytes();
        for byte in bytes {
            self.write(byte, span.clone());
        }
    }

    /// Write an opcode with two u8 operands
    pub fn write_opcode_u8_u8(&mut self, opcode: Opcode, op1: u8, op2: u8, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        self.write(op1, span.clone());
        self.write(op2, span);
    }

    /// Write an opcode with u16 and u8 operands
    pub fn write_opcode_u16_u8(&mut self, opcode: Opcode, op1: u16, op2: u8, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        let bytes1 = op1.to_le_bytes();
        self.write(bytes1[0], span.clone());
        self.write(bytes1[1], span.clone());
        self.write(op2, span);
    }

    /// Write an opcode with u8 and u32 operands
    pub fn write_opcode_u8_u32(&mut self, opcode: Opcode, op1: u8, op2: u32, span: Range<usize>) {
        self.write(opcode as u8, span.clone());
        self.write(op1, span.clone());
        let bytes2 = op2.to_le_bytes();
        for byte in bytes2 {
            self.write(byte, span.clone());
        }
    }

    /// Read a u8 from the code at the given index
    pub fn read_u8(&self, index: usize) -> Option<u8> {
        self.code.get(index).copied()
    }

    /// Read a u16 from the code at the given index (little-endian)
    pub fn read_u16(&self, index: usize) -> Option<u16> {
        if index + 1 < self.code.len() {
            let bytes = [self.code[index], self.code[index + 1]];
            Some(u16::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Read a u32 from the code at the given index (little-endian)
    pub fn read_u32(&self, index: usize) -> Option<u32> {
        if index + 3 < self.code.len() {
            let bytes = [
                self.code[index],
                self.code[index + 1],
                self.code[index + 2],
                self.code[index + 3],
            ];
            Some(u32::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Read an i32 from the code at the given index (little-endian)
    pub fn read_i32(&self, index: usize) -> Option<i32> {
        if index + 3 < self.code.len() {
            let bytes = [
                self.code[index],
                self.code[index + 1],
                self.code[index + 2],
                self.code[index + 3],
            ];
            Some(i32::from_le_bytes(bytes))
        } else {
            None
        }
    }

    /// Read an opcode from the code at the given index
    pub fn read_opcode(&self, index: usize) -> Option<Opcode> {
        self.code.get(index).and_then(|&byte| Opcode::from_u8(byte))
    }

    /// Creates a debug compilation report showing all opcodes with their spans in different colors
    pub fn debug_compilation(&self) -> Report<(&str, Range<usize>)> {
        // Build raw bytecode display
        let mut raw_bytecode = String::from("Raw Bytecode:\n");
        for (i, byte) in self.code.iter().enumerate() {
            raw_bytecode.push_str(&format!("[{}]: {:02X} ", i, byte));
            if (i + 1) % 8 == 0 {
                raw_bytecode.push('\n');
            }
        }
        if !self.code.is_empty() && self.code.len() % 8 != 0 {
            raw_bytecode.push('\n');
        }

        let mut report = Report::build(ReportKind::Advice, (self.source_id, 0..0))
            .with_message(format!("Debug: Compilation bytecode visualization\n\n{}", raw_bytecode));

        // Color palette for different opcodes
        let colors = [
            ariadne::Color::Primary,
            ariadne::Color::Green,
            ariadne::Color::Blue,
            ariadne::Color::Cyan,
            ariadne::Color::Magenta,
            ariadne::Color::Yellow,
        ];

        let mut ip = 0; // instruction pointer
        let mut color_index = 0;

        while ip < self.code.len() {
            if let Some(opcode) = self.read_opcode(ip) {
                let span = self.get_span(ip);
                let color = colors[color_index % colors.len()];
                color_index += 1;

                // Calculate instruction size and end position
                let instruction_size = match opcode {
                    Opcode::LoadConst => 3, // opcode + u16
                    Opcode::LoadVar => 3,   // opcode + u16
                    Opcode::CreateObject => 3, // opcode + u16
                    Opcode::CreateArray => 3,  // opcode + u16
                    Opcode::FieldDef => 4,     // opcode + u16 + u8
                    Opcode::CreateFunction => 6, // opcode + u8 + u32
                    Opcode::Call => 3,         // opcode + u8 + u8
                    Opcode::Jump | Opcode::JumpIfFalse | Opcode::JumpIfTrue => 5, // opcode + i32
                    Opcode::LocalScope => 2,   // opcode + u8
                    Opcode::StdCall => 4,      // opcode + u16 + u8
                    Opcode::BindDefault => 3,  // opcode + u16
                    // All other opcodes have no operands
                    _ => 1,
                };
                let end_pos = ip + instruction_size - 1;

                // Create a label for this opcode with bytecode range and operand details
                let label_text = match opcode {
                    Opcode::LoadConst => {
                        if let Some(const_index) = self.read_u16(ip + 1) {
                            if let Some(value) = self.constants.get(const_index as usize) {
                                format!("[{}-{}] ({} bytes) LoadConst: opcode={:02X}@{}, operand={:04X}@{}-{}, value={}",
                                    ip, end_pos, instruction_size, opcode as u8, ip, const_index, ip + 1, ip + 2, value)
                            } else {
                                format!("[{}-{}] ({} bytes) LoadConst: opcode={:02X}@{}, operand={:04X}@{}-{}",
                                    ip, end_pos, instruction_size, opcode as u8, ip, const_index, ip + 1, ip + 2)
                            }
                        } else {
                            format!("[{}-{}] ({} bytes) LoadConst: opcode={:02X}@{}",
                                ip, end_pos, instruction_size, opcode as u8, ip)
                        }
                    }
                    _ => {
                        if instruction_size == 1 {
                            format!("[{}] (1 byte) {}: opcode={:02X}@{}", ip, format!("{:?}", opcode), opcode as u8, ip)
                        } else {
                            format!("[{}-{}] ({} bytes) {}: opcode={:02X}@{}",
                                ip, end_pos, instruction_size, format!("{:?}", opcode), opcode as u8, ip)
                        }
                    }
                };

                if let Some(span) = span {
                    report = report.with_label(
                        Label::new((self.source_id, span.clone()))
                            .with_message(label_text)
                            .with_color(color)
                    );
                }

                // Move instruction pointer by the instruction size
                ip += instruction_size;
            } else {
                // Invalid opcode, skip
                ip += 1;
            }
        }

        report.finish()
    }
}

impl<'a> Default for Chunk<'a> {
    fn default() -> Self {
        Self::new("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_chunk() {
        let chunk = Chunk::new("test.jsonnet");
        assert!(chunk.is_empty());
        assert_eq!(chunk.count(), 0);
        assert_eq!(chunk.code.len(), 0);
        assert_eq!(chunk.spans.len(), 0);
        assert_eq!(chunk.constants.len(), 0);
        assert_eq!(chunk.source_id, "test.jsonnet");
    }

    #[test]
    fn test_write_single_instruction() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);

        assert_eq!(chunk.count(), 1);
        assert_eq!(chunk.code[0], 123);
        assert_eq!(chunk.spans.len(), 1);
        assert_eq!(chunk.spans[0].span, 0..5);
        assert_eq!(chunk.spans[0].repeated_values, 1);
    }

    #[test]
    fn test_write_multiple_instructions_different_spans() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);
        chunk.write(124, 5..10);
        chunk.write(125, 10..15);

        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.spans.len(), 3);
        assert_eq!(chunk.spans[0].span, 0..5);
        assert_eq!(chunk.spans[0].repeated_values, 1);
        assert_eq!(chunk.spans[1].span, 5..10);
        assert_eq!(chunk.spans[1].repeated_values, 1);
        assert_eq!(chunk.spans[2].span, 10..15);
        assert_eq!(chunk.spans[2].repeated_values, 1);
    }

    #[test]
    fn test_write_same_span() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);
        chunk.write(124, 0..5);
        chunk.write(125, 0..5);

        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.spans.len(), 1);
        assert_eq!(chunk.spans[0].span, 0..5);
        assert_eq!(chunk.spans[0].repeated_values, 3);
    }

    #[test]
    fn test_write_mixed_spans() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);
        chunk.write(124, 5..10);
        chunk.write(125, 5..10);
        chunk.write(126, 10..15);

        assert_eq!(chunk.count(), 4);
        assert_eq!(chunk.spans.len(), 3);

        assert_eq!(chunk.spans[0].span, 0..5);
        assert_eq!(chunk.spans[0].repeated_values, 1);

        assert_eq!(chunk.spans[1].span, 5..10);
        assert_eq!(chunk.spans[1].repeated_values, 2);

        assert_eq!(chunk.spans[2].span, 10..15);
        assert_eq!(chunk.spans[2].repeated_values, 1);
    }

    #[test]
    fn test_get_span() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);
        chunk.write(124, 0..5);
        chunk.write(125, 5..10);
        chunk.write(126, 10..15);
        chunk.write(127, 10..15);

        assert_eq!(chunk.get_span(0), Some(&(0..5)));
        assert_eq!(chunk.get_span(1), Some(&(0..5)));
        assert_eq!(chunk.get_span(2), Some(&(5..10)));
        assert_eq!(chunk.get_span(3), Some(&(10..15)));
        assert_eq!(chunk.get_span(4), Some(&(10..15)));
        assert_eq!(chunk.get_span(5), None);
    }

    #[test]
    fn test_add_constant() {
        let mut chunk = Chunk::new("test.jsonnet");

        let index1 = chunk.add_constant(Value::Number(1.5));
        let index2 = chunk.add_constant(Value::Number(2.7));
        let index3 = chunk.add_constant(Value::Number(3.14));

        assert_eq!(index1, 0);
        assert_eq!(index2, 1);
        assert_eq!(index3, 2);

        assert_eq!(chunk.constants[0], Value::Number(1.5));
        assert_eq!(chunk.constants[1], Value::Number(2.7));
        assert_eq!(chunk.constants[2], Value::Number(3.14));
    }

    #[test]
    fn test_span_run_length() {
        let span_info = SpanRunLength::new(42..84, 5);
        assert_eq!(span_info.span, 42..84);
        assert_eq!(span_info.repeated_values, 5);
    }

    #[test]
    fn test_create_error_report() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write(123, 0..5);
        chunk.write(124, 5..10);
        chunk.write(125, 10..15);

        let report = chunk.create_error_report(1..3, "Test compilation error");

        // The report should be created successfully - we can't easily test the internal
        // structure without making the test too brittle, but we can verify it was created
        // by checking it's the right type (this will compile if the function works)
        let _: Report<(&str, Range<usize>)> = report;
    }

    #[test]
    fn test_opcode_conversion() {
        assert_eq!(Opcode::from_u8(0), Some(Opcode::LoadNull));
        assert_eq!(Opcode::from_u8(3), Some(Opcode::LoadConst));
        assert_eq!(Opcode::from_u8(50), Some(Opcode::Add));
        assert_eq!(Opcode::from_u8(255), None);
    }

    #[test]
    fn test_write_opcode() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode(Opcode::LoadNull, 0..5);

        assert_eq!(chunk.count(), 1);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::LoadNull));
    }

    #[test]
    fn test_write_opcode_u16() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u16(Opcode::LoadConst, 0x1234, 0..5);

        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::LoadConst));
        assert_eq!(chunk.read_u16(1), Some(0x1234));
    }

    #[test]
    fn test_write_opcode_u32() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u32(Opcode::CreateFunction, 0x12345678, 0..5);

        assert_eq!(chunk.count(), 5);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::CreateFunction));
        assert_eq!(chunk.read_u32(1), Some(0x12345678));
    }

    #[test]
    fn test_write_opcode_i32() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_i32(Opcode::Jump, -42, 0..5);

        assert_eq!(chunk.count(), 5);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::Jump));
        assert_eq!(chunk.read_i32(1), Some(-42));
    }

    #[test]
    fn test_write_opcode_u8_u8() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u8_u8(Opcode::Call, 3, 2, 0..5);

        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::Call));
        assert_eq!(chunk.read_u8(1), Some(3));
        assert_eq!(chunk.read_u8(2), Some(2));
    }

    #[test]
    fn test_write_opcode_u16_u8() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u16_u8(Opcode::FieldDef, 0x1234, 1, 0..5);

        assert_eq!(chunk.count(), 4);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::FieldDef));
        assert_eq!(chunk.read_u16(1), Some(0x1234));
        assert_eq!(chunk.read_u8(3), Some(1));
    }

    #[test]
    fn test_write_opcode_u8_u32() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u8_u32(Opcode::CreateFunction, 5, 0x12345678, 0..5);

        assert_eq!(chunk.count(), 6);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::CreateFunction));
        assert_eq!(chunk.read_u8(1), Some(5));
        assert_eq!(chunk.read_u32(2), Some(0x12345678));
    }

    #[test]
    fn test_field_visibility() {
        assert_eq!(FieldVisibility::Visible as u8, 0);
        assert_eq!(FieldVisibility::Hidden as u8, 1);
        assert_eq!(FieldVisibility::ForceVisible as u8, 2);
    }

    #[test]
    fn test_complex_opcode_sequence() {
        let mut chunk = Chunk::new("test.jsonnet");

        // Simulate: LOAD_CONST 0, ADD, RETURN
        chunk.write_opcode_u16(Opcode::LoadConst, 0, 0..5);  // 3 bytes: opcode + u16
        chunk.write_opcode(Opcode::Add, 5..10);              // 1 byte: opcode
        chunk.write_opcode(Opcode::Return, 10..15);          // 1 byte: opcode

        assert_eq!(chunk.count(), 5);
        assert_eq!(chunk.read_opcode(0), Some(Opcode::LoadConst));
        assert_eq!(chunk.read_u16(1), Some(0));
        assert_eq!(chunk.read_opcode(3), Some(Opcode::Add));
        assert_eq!(chunk.read_opcode(4), Some(Opcode::Return));
    }

    #[test]
    fn test_read_beyond_bounds() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode(Opcode::LoadNull, 0..5);

        assert_eq!(chunk.read_u16(0), None);
        assert_eq!(chunk.read_u32(0), None);
        assert_eq!(chunk.read_opcode(5), None);
    }

    #[test]
    fn test_default() {
        let chunk = Chunk::default();
        assert!(chunk.is_empty());
        assert_eq!(chunk.count(), 0);
        assert_eq!(chunk.source_id, "");
    }

    #[test]
    fn test_value_display() {
        // Test null
        assert_eq!(format!("{}", Value::Null), "null");

        // Test boolean
        assert_eq!(format!("{}", Value::Boolean(true)), "true");
        assert_eq!(format!("{}", Value::Boolean(false)), "false");

        // Test number
        assert_eq!(format!("{}", Value::Number(42.0)), "42");
        assert_eq!(format!("{}", Value::Number(-3.14)), "-3.14");
        assert_eq!(format!("{}", Value::Number(0.0)), "0");

        // Test special number values
        assert_eq!(format!("{}", Value::Number(f64::NAN)), "NaN");
        assert_eq!(format!("{}", Value::Number(f64::INFINITY)), "Infinity");
        assert_eq!(format!("{}", Value::Number(f64::NEG_INFINITY)), "-Infinity");

        // Test String and Object with indices (using slotmap key)
        use slotmap::SlotMap;
        let mut string_map: SlotMap<StringIndex, String> = SlotMap::new();
        let mut object_map: SlotMap<ObjectIndex, String> = SlotMap::new();

        let string_key = string_map.insert("test".to_string());
        let object_key = object_map.insert("test_object".to_string());

        let string_display = format!("{}", Value::String(string_key));
        let object_display = format!("{}", Value::Object(object_key));

        // Verify format - should be "String[<debug_output>]" and "Object[<debug_output>]"
        assert!(string_display.starts_with("String["));
        assert!(string_display.ends_with("]"));
        assert!(object_display.starts_with("Object["));
        assert!(object_display.ends_with("]"));
    }
}
