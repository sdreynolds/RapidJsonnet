/// A type alias for values in the chunk - currently only f64
pub type Value = f64;

/// Represents a run-length encoding for line numbers
/// This struct maps code indices to their corresponding source code line numbers
/// in an efficient way by storing only unique lines and their repetition counts
#[derive(Debug, Clone, PartialEq)]
pub struct LineRunLength {
    /// The line number in the source code
    pub line: usize,
    /// The count of opcodes/operands that share the same line
    pub repeated_values: usize,
}

impl LineRunLength {
    /// Creates a new LineRunLength entry
    pub fn new(line: usize, repeated_values: usize) -> Self {
        Self {
            line,
            repeated_values,
        }
    }
}

/// A chunk represents a collection of bytecode instructions and associated metadata
/// for the virtual machine to execute
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Vector of bytecode containing opcodes and operands
    pub code: Vec<u8>,
    /// Vector mapping code indices to line numbers using run-length encoding
    pub lines: Vec<LineRunLength>,
    /// Vector of constant values referenced by the bytecode
    pub constants: Vec<Value>,
}

impl Chunk {
    /// Creates a new empty chunk
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            lines: Vec::new(),
            constants: Vec::new(),
        }
    }

    /// Writes a byte to the chunk's code at the specified line number
    pub fn write(&mut self, byte: u8, line: usize) {
        self.code.push(byte);

        // Update line information using run-length encoding
        if let Some(last_line) = self.lines.last_mut() {
            if last_line.line == line {
                // Same line as previous instruction, increment count
                last_line.repeated_values += 1;
            } else {
                // New line, create new entry
                self.lines.push(LineRunLength::new(line, 1));
            }
        } else {
            // First instruction
            self.lines.push(LineRunLength::new(line, 1));
        }
    }

    /// Adds a constant value to the chunk and returns its index
    pub fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    /// Gets the line number for a given instruction index
    pub fn get_line(&self, instruction_index: usize) -> Option<usize> {
        let mut current_index = 0;

        for line_info in &self.lines {
            if instruction_index < current_index + line_info.repeated_values {
                return Some(line_info.line);
            }
            current_index += line_info.repeated_values;
        }

        None
    }

    /// Returns the number of instructions in the chunk
    pub fn count(&self) -> usize {
        self.code.len()
    }

    /// Returns whether the chunk is empty
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_chunk() {
        let chunk = Chunk::new();
        assert!(chunk.is_empty());
        assert_eq!(chunk.count(), 0);
        assert_eq!(chunk.code.len(), 0);
        assert_eq!(chunk.lines.len(), 0);
        assert_eq!(chunk.constants.len(), 0);
    }

    #[test]
    fn test_write_single_instruction() {
        let mut chunk = Chunk::new();
        chunk.write(123, 1);

        assert_eq!(chunk.count(), 1);
        assert_eq!(chunk.code[0], 123);
        assert_eq!(chunk.lines.len(), 1);
        assert_eq!(chunk.lines[0].line, 1);
        assert_eq!(chunk.lines[0].repeated_values, 1);
    }

    #[test]
    fn test_write_same_line() {
        let mut chunk = Chunk::new();
        chunk.write(123, 1);
        chunk.write(124, 1);
        chunk.write(125, 1);

        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.lines.len(), 1);
        assert_eq!(chunk.lines[0].line, 1);
        assert_eq!(chunk.lines[0].repeated_values, 3);
    }

    #[test]
    fn test_write_different_lines() {
        let mut chunk = Chunk::new();
        chunk.write(123, 1);
        chunk.write(124, 2);
        chunk.write(125, 2);
        chunk.write(126, 3);

        assert_eq!(chunk.count(), 4);
        assert_eq!(chunk.lines.len(), 3);

        assert_eq!(chunk.lines[0].line, 1);
        assert_eq!(chunk.lines[0].repeated_values, 1);

        assert_eq!(chunk.lines[1].line, 2);
        assert_eq!(chunk.lines[1].repeated_values, 2);

        assert_eq!(chunk.lines[2].line, 3);
        assert_eq!(chunk.lines[2].repeated_values, 1);
    }

    #[test]
    fn test_get_line() {
        let mut chunk = Chunk::new();
        chunk.write(123, 1);
        chunk.write(124, 1);
        chunk.write(125, 2);
        chunk.write(126, 3);
        chunk.write(127, 3);

        assert_eq!(chunk.get_line(0), Some(1));
        assert_eq!(chunk.get_line(1), Some(1));
        assert_eq!(chunk.get_line(2), Some(2));
        assert_eq!(chunk.get_line(3), Some(3));
        assert_eq!(chunk.get_line(4), Some(3));
        assert_eq!(chunk.get_line(5), None);
    }

    #[test]
    fn test_add_constant() {
        let mut chunk = Chunk::new();

        let index1 = chunk.add_constant(1.5);
        let index2 = chunk.add_constant(2.7);
        let index3 = chunk.add_constant(3.14);

        assert_eq!(index1, 0);
        assert_eq!(index2, 1);
        assert_eq!(index3, 2);

        assert_eq!(chunk.constants[0], 1.5);
        assert_eq!(chunk.constants[1], 2.7);
        assert_eq!(chunk.constants[2], 3.14);
    }

    #[test]
    fn test_line_run_length() {
        let line_info = LineRunLength::new(42, 5);
        assert_eq!(line_info.line, 42);
        assert_eq!(line_info.repeated_values, 5);
    }

    #[test]
    fn test_default() {
        let chunk = Chunk::default();
        assert!(chunk.is_empty());
        assert_eq!(chunk.count(), 0);
    }
}
