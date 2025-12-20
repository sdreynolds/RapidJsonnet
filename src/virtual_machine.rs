use chunk::{
    Chunk, ClosureIndex, I32_SIZE_BYTES, OPCODE_SIZE_BYTES, ObjectIndex, Opcode, OwnedChunk,
    RuntimeError, Value,
};
use memory_manager::{MemoryManager, UpvalueIndex};
use std::ops::Range;

/// Maximum number of nested function calls
const MAX_FRAMES: usize = 256;

/// Represents a function call frame on the call stack
#[derive(Debug, Clone, Copy)]
pub struct CallFrame {
    /// The closure being executed in this frame
    pub closure: ClosureIndex,
    /// Instruction pointer within the frame's function
    pub ip: usize,
    /// Base position in VM stack where this frame's locals begin
    pub stack_base: usize,
}

impl CallFrame {
    /// Create a new call frame
    pub fn new(closure: ClosureIndex, ip: usize, stack_base: usize) -> Self {
        Self {
            closure,
            ip,
            stack_base,
        }
    }
}

/// Virtual machine for executing Jsonnet bytecode
pub struct VirtualMachine {
    /// Call frame stack for function calls
    frames: Vec<CallFrame>,
    /// Number of active frames (logical size of frames vec)
    frame_count: usize,
    /// Linked list of open upvalues (pointing to stack locations)
    open_upvalues: Option<UpvalueIndex>,
    /// Execution stack
    stack: Vec<Value>,
    /// String pool for string interning and GC
    memory_manager: MemoryManager,
}

impl VirtualMachine {
    /// Create a new virtual machine with the given starting chunk and string pool
    pub fn new(chunk: Chunk, mut memory_manager: MemoryManager) -> Self {
        // Convert chunk to owned chunk for function storage
        let owned_chunk = chunk.into_owned();

        // Create a top-level function from the chunk
        let func_result = memory_manager.allocate_function(None, 0, 0, owned_chunk);

        // Create a closure wrapping the top-level function
        let closure_result = memory_manager.allocate_closure(func_result.index, Vec::new());

        // Create initial frame with the top-level closure
        let initial_frame = CallFrame::new(closure_result.index, 0, 0);

        let mut frames = Vec::with_capacity(MAX_FRAMES);
        frames.push(initial_frame);

        Self {
            frames,
            frame_count: 1,
            open_upvalues: None,
            stack: Vec::with_capacity(1024),
            memory_manager,
        }
    }

    /// Get the current call frame
    fn current_frame(&self) -> &CallFrame {
        &self.frames[self.frame_count - 1]
    }

    /// Get mutable reference to the current call frame
    fn current_frame_mut(&mut self) -> &mut CallFrame {
        &mut self.frames[self.frame_count - 1]
    }

    /// Get the current chunk being executed from the current frame's closure
    fn current_chunk(&self) -> &OwnedChunk {
        let frame = self.current_frame();
        let closure = self.memory_manager.load_closure(frame.closure);
        let function = self.memory_manager.load_function(closure.function);
        &function.chunk
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
        let frame = self.current_frame();
        self.current_chunk()
            .get_span(frame.ip)
            .cloned()
            .unwrap_or(0..0)
    }

    /// Read a u16 operand from the current position and advance PC
    fn read_u16_operand(&mut self) -> Result<u16, RuntimeError> {
        let frame = self.current_frame();
        let chunk = self.current_chunk();
        let operand = chunk.read_u16(frame.ip + 1).ok_or_else(|| RuntimeError {
            span: self.get_current_span(),
            message: "Invalid bytecode - missing operand".to_string(),
            source_id: chunk.source_id.to_string(),
        })?;

        self.current_frame_mut().ip += 3; // opcode + 2 bytes for u16
        Ok(operand)
    }

    /// Read a i32 operand from the current position and advance PC
    fn read_i32_operand(&mut self) -> Result<i32, RuntimeError> {
        let frame = self.current_frame();
        let chunk = self.current_chunk();
        let operand = chunk
            .read_i32(frame.ip + OPCODE_SIZE_BYTES)
            .ok_or_else(|| RuntimeError {
                span: self.get_current_span(),
                message: "Invalid bytecode - missing i32 operand".to_string(),
                source_id: chunk.source_id.to_string(),
            })?;

        self.current_frame_mut().ip += OPCODE_SIZE_BYTES + I32_SIZE_BYTES;
        Ok(operand)
    }

    /// Advance program counter by 1 (for opcodes with no operands)
    fn advance_pc(&mut self) {
        self.current_frame_mut().ip += 1;
    }

    /// Capture an upvalue for the given stack location.
    /// If an upvalue already exists for this location, returns the existing one.
    /// Otherwise, creates a new upvalue and inserts it into the open_upvalues linked list.
    fn capture_upvalue(&mut self, stack_location: usize) -> UpvalueIndex {
        // Walk through the open_upvalues linked list
        let mut prev_upvalue: Option<UpvalueIndex> = None;
        let mut current_upvalue = self.open_upvalues;

        // Find the position to insert or return existing upvalue
        while let Some(upvalue_index) = current_upvalue {
            let upvalue = self.memory_manager.load_upvalue(upvalue_index);

            if let Some(location) = upvalue.stack_location {
                if location == stack_location {
                    // Found existing upvalue for this location
                    return upvalue_index;
                }

                if location < stack_location {
                    // We've passed the location where this upvalue should be
                    break;
                }
            }

            prev_upvalue = Some(upvalue_index);
            current_upvalue = upvalue.next;
        }

        // Create a new upvalue
        let upvalue_allocation = self.memory_manager.allocate_upvalue(stack_location);
        let new_upvalue_index = upvalue_allocation.index;

        // Insert the new upvalue into the linked list
        if let Some(prev) = prev_upvalue {
            // Insert after prev
            let current_next = self.memory_manager.load_upvalue(prev).next;
            self.memory_manager.load_upvalue_mut(new_upvalue_index).next = current_next;
            self.memory_manager.load_upvalue_mut(prev).next = Some(new_upvalue_index);
        } else {
            // Insert at the head of the list
            self.memory_manager.load_upvalue_mut(new_upvalue_index).next = self.open_upvalues;
            self.open_upvalues = Some(new_upvalue_index);
        }

        if upvalue_allocation.should_garbage_collect {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!(
                    "[VirtualMachine] Running GC at PC={} (Upvalue allocation)",
                    self.current_frame().ip
                );
            }
            self.run_garbage_collection();
        }

        new_upvalue_index
    }

    /// Main interpretation loop
    pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
        loop {
            let frame = self.current_frame();
            let chunk = self.current_chunk();

            // Check if we've reached the end
            if frame.ip >= chunk.count() {
                return Err(RuntimeError {
                    span: self.get_current_span(),
                    message: "Unexpected end of bytecode - missing Return instruction".to_string(),
                    source_id: chunk.source_id.to_string(),
                });
            }

            let opcode = chunk.read_opcode(frame.ip).ok_or_else(|| RuntimeError {
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

                    let constant = chunk.constants[index as usize].clone();
                    self.push(constant)?;
                }

                Opcode::LoadVar => {
                    let slot_offset = self.read_u16_operand()? as usize;
                    let frame = self.current_frame();

                    // Calculate absolute stack position: frame base + slot offset
                    let stack_slot = frame.stack_base + slot_offset;

                    // Validate stack slot
                    if stack_slot >= self.stack.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "Invalid stack slot {} (stack size: {})",
                                stack_slot,
                                self.stack.len()
                            ),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }

                    // Copy value from slot to top of stack
                    let value = self.stack[stack_slot].clone();
                    self.push(value)?;
                }

                // Binary arithmetic operations
                Opcode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;

                    // Check for different addition types
                    match (&a, &b) {
                        // Object merging (according to Jsonnet spec)
                        (Value::Object(left_key), Value::Object(right_key)) => {
                            let (left_object, right_object) = (
                                self.memory_manager.load_object(*left_key),
                                self.memory_manager.load_object(*right_key),
                            );
                            // Create merged properties starting with left object
                            let mut merged_properties = left_object.properties.clone();

                            // Override/add properties from right object
                            for (key, value) in &right_object.properties {
                                merged_properties.insert(*key, value.clone());
                            }

                            let merged_allocation = self
                                .memory_manager
                                .allocate_object_with_properties(merged_properties);
                            self.push(Value::Object(merged_allocation.index))?;
                            if merged_allocation.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (Object merge in Concat)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        (Value::Object(_), _) | (_, Value::Object(_)) => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Must concatenate objects with other objects".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                        // Array concatenation
                        (Value::Array(left_key), Value::Array(right_key)) => {
                            let (left_array, right_array) = (
                                self.memory_manager.load_array(*left_key),
                                self.memory_manager.load_array(*right_key),
                            );

                            // Create concatenated elements
                            let mut concatenated =
                                Vec::with_capacity(left_array.len() + right_array.len());
                            concatenated.extend_from_slice(&left_array.elements);
                            concatenated.extend_from_slice(&right_array.elements);

                            // Allocate new concatenated array
                            let concat_allocation =
                                self.memory_manager.allocate_array(concatenated);
                            self.push(Value::Array(concat_allocation.index))?;

                            if concat_allocation.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (Array concatenation)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        (Value::Array(_), _) | (_, Value::Array(_)) => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Must concatenate arrays with other arrays".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                        // String concatenation if either operand is a string
                        (Value::String(_), _) | (_, Value::String(_)) => {
                            let a_str = match &a {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_)
                                | Value::Array(_)
                                | Value::Function(_)
                                | Value::Closure(_) => unreachable!(),
                            };
                            let b_str = match &b {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_)
                                | Value::Array(_)
                                | Value::Function(_)
                                | Value::Closure(_) => unreachable!(),
                            };
                            let result_str = format!("{}{}", a_str, b_str);
                            let interned = self.memory_manager.allocate_string(&result_str);
                            self.push(Value::String(interned.index))?;
                            if interned.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (String concat in Concat fallback)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        // Numeric addition for all other cases
                        _ => {
                            let result = self.to_number(a)? + self.to_number(b)?;
                            self.push(Value::Number(result))?;
                        }
                    }
                    self.advance_pc();
                }

                Opcode::Sub => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? - self.to_number(b)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Mul => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? * self.to_number(b)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Div => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let b_num = self.to_number(b)?;
                    if b_num == 0.0 {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Division by zero".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    let result = self.to_number(a)? / b_num;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Comparison operations
                Opcode::Lt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? < self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Le => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? <= self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Gt => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? > self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ge => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.to_number(a)? >= self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                // Equality operations
                Opcode::Eq => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = self.values_equal(&a, &b);
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ne => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = !self.values_equal(&a, &b);
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                // String operations
                Opcode::StringConcat => {
                    let b = self.pop()?;
                    let a = self.pop()?;

                    // @TODO: actually benchmark this optimization. First glance doesn't look like an improvement.
                    // Optimized string concatenation - assumes both are strings
                    match (&a, &b) {
                        (Value::String(s1), Value::String(s2)) => {
                            let result_str = format!(
                                "{}{}",
                                self.memory_manager.load_string(*s1),
                                self.memory_manager.load_string(*s2)
                            );
                            let interned = self.memory_manager.allocate_string(&result_str);
                            self.push(Value::String(interned.index))?;
                            if interned.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (String concat in StringConcat)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        _ => {
                            // This should not happen if compiler type tracking is correct
                            // Fall back to safe conversion
                            let a_str = match &a {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                                Value::Array(_) => "{array}".to_string(),
                                Value::Function(_) => "{function}".to_string(),
                                Value::Closure(_) => "{closure}".to_string(),
                            };
                            let b_str = match &b {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                                Value::Array(_) => "{array}".to_string(),
                                Value::Function(_) => "{function}".to_string(),
                                Value::Closure(_) => "{closure}".to_string(),
                            };
                            let result_str = format!("{}{}", a_str, b_str);
                            let interned = self.memory_manager.allocate_string(&result_str);
                            self.push(Value::String(interned.index))?;
                            if interned.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (String concat in Multiply)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                    };
                    self.advance_pc();
                }

                // Bitwise operations
                Opcode::Shl => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let shift_count = (self.to_integer(b)? % 64) as u32;
                    if shift_count >= 64 {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Invalid shift count".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    let result = (self.to_integer(a)? << shift_count) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Shr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let shift_count = (self.to_integer(b)? % 64) as u32;
                    let result = (self.to_integer(a)? >> shift_count) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitAnd => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (self.to_integer(a)? & self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitXor => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (self.to_integer(a)? ^ self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitOr => {
                    let b = self.pop()?;
                    let a = self.pop()?;
                    let result = (self.to_integer(a)? | self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Unary operations
                Opcode::Neg => {
                    let a = self.pop()?;
                    let result = -self.to_number(a)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Pos => {
                    let a = self.pop()?;
                    let result = self.to_number(a)?; // Unary + is essentially a no-op for numbers
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Not => {
                    let a = self.pop()?;
                    let result = !self.is_truthy(a);
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::BitNot => {
                    let a = self.pop()?;
                    let result = (!self.to_integer(a)?) as f64;
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

                Opcode::CreateObject => {
                    let field_count = self.read_u16_operand()?;

                    // Pop field_count pairs of (key, value) from the stack
                    let mut properties = std::collections::HashMap::new();

                    for _ in 0..field_count {
                        let value = self.pop()?;
                        let key = self.pop()?;

                        // Ensure key is a string
                        if let Value::String(key_str) = key {
                            properties.insert(key_str, value);
                        } else {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("Object key must be a string, got {:?}", key),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }

                    let object_allocation = self
                        .memory_manager
                        .allocate_object_with_properties(properties);
                    self.push(Value::Object(object_allocation.index))?;
                    if object_allocation.should_garbage_collect {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[VirtualMachine] Running GC at PC={} (Object construction)",
                                self.current_frame().ip
                            );
                        }
                        self.run_garbage_collection();
                    }
                }

                Opcode::CreateArray => {
                    let element_count = self.read_u16_operand()? as usize;

                    // Pre-allocate vector and fill backwards (per docs/arrays.md)
                    let mut elements = Vec::with_capacity(element_count);
                    elements.resize(element_count, Value::Null);

                    for i in (0..element_count).rev() {
                        elements[i] = self.pop()?;
                    }

                    // Allocate array in memory manager
                    let array_allocation = self.memory_manager.allocate_array(elements);
                    self.push(Value::Array(array_allocation.index))?;

                    // Check if GC should run
                    if array_allocation.should_garbage_collect {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[VirtualMachine] Running GC at PC={} (Array construction)",
                                self.current_frame().ip
                            );
                        }
                        self.run_garbage_collection();
                    }
                }

                Opcode::ObjectIndex => {
                    let field_name = self.pop()?; // Property name to access
                    let object_value = self.pop()?; // Object to index into

                    // Ensure we have an object
                    if let Value::Object(object_key) = object_value {
                        // Ensure field name is a string
                        if let Value::String(field_key) = field_name {
                            let object = self.memory_manager.load_object(object_key);
                            if let Some(value) = object.get(&field_key) {
                                self.push(value.clone())?;
                            } else {
                                // Property doesn't exist, push null
                                self.push(Value::Null)?;
                            }
                        } else {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Object index must be a string, got {:?}",
                                    field_name
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "Cannot index into non-object value: {:?}",
                                object_value
                            ),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    self.advance_pc();
                }

                Opcode::ArrayIndex => {
                    let index_value = self.pop()?;
                    let container_value = self.pop()?;

                    match container_value {
                        Value::Array(array_key) => {
                            // Array indexing with number
                            if let Value::Number(index_num) = index_value {
                                // Check for negative index
                                if index_num < 0.0 {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Array index cannot be negative, got {}",
                                            index_num
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }

                                // Check for non-integer index
                                if index_num.fract() != 0.0 {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Array index must be an integer, got {}",
                                            index_num
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }

                                let index = index_num as usize;
                                let array = self.memory_manager.load_array(array_key);

                                // Bounds check
                                if index >= array.len() {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Array index {} out of bounds (length: {})",
                                            index,
                                            array.len()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }

                                self.push(array.elements[index])?;
                            } else {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "Array index must be a number, got {:?}",
                                        index_value
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        }
                        Value::Object(object_key) => {
                            // Object indexing with string
                            if let Value::String(field_key) = index_value {
                                let object = self.memory_manager.load_object(object_key);
                                if let Some(value) = object.get(&field_key) {
                                    self.push(value.clone())?;
                                } else {
                                    self.push(Value::Null)?;
                                }
                            } else {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "Object index must be a string, got {:?}",
                                        index_value
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        }
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("Cannot index into value: {:?}", container_value),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }

                    self.advance_pc();
                }

                Opcode::ObjectMerge => {
                    let right_value = self.pop()?; // Right-hand side object
                    let left_value = self.pop()?; // Left-hand side object

                    // Ensure both values are objects
                    if let (Value::Object(left_key), Value::Object(right_key)) =
                        (left_value, right_value)
                    {
                        let (left_object, right_object) = (
                            self.memory_manager.load_object(left_key),
                            self.memory_manager.load_object(right_key),
                        );
                        // Create merged properties starting with left object
                        let mut merged_properties = left_object.properties.clone();

                        // Override/add properties from right object
                        for (key, value) in &right_object.properties {
                            merged_properties.insert(*key, value.clone());
                        }

                        let merged_allocation = self
                            .memory_manager
                            .allocate_object_with_properties(merged_properties);
                        self.push(Value::Object(merged_allocation.index))?;
                        if merged_allocation.should_garbage_collect {
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!(
                                    "[VirtualMachine] Running GC at PC={} (Object merge in Add)",
                                    self.current_frame().ip
                                );
                            }
                            self.run_garbage_collection();
                        }
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Object merge requires two objects".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    self.advance_pc();
                }

                Opcode::Error => {
                    // Pop the error message value from the stack
                    let error_value = self.pop()?;

                    // Convert the value to string using existing JSON conversion
                    let mut visited = std::collections::HashSet::new();
                    let json_value = self.value_to_json(&error_value, &mut visited)?;
                    let error_message = json_value.to_string();

                    // Return RuntimeError with the converted message
                    return Err(RuntimeError {
                        span: self.get_current_span(),
                        message: error_message,
                        source_id: self.current_chunk().source_id.to_string(),
                    });
                }

                // Jump opcodes
                Opcode::Jump => {
                    let offset = self.read_i32_operand()?;
                    // IP is already advanced by read_i32_operand, adjust by offset
                    let frame = self.current_frame_mut();
                    frame.ip = (frame.ip as i32 + offset) as usize;
                }

                Opcode::JumpIfFalse => {
                    let offset = self.read_i32_operand()?;
                    let condition = self.pop()?;
                    if !self.is_truthy(condition) {
                        let frame = self.current_frame_mut();
                        frame.ip = (frame.ip as i32 + offset) as usize;
                    }
                    // If truthy, IP already advanced past jump instruction
                }

                Opcode::JumpIfTrue => {
                    let offset = self.read_i32_operand()?;
                    let condition = self.pop()?;
                    if self.is_truthy(condition) {
                        let frame = self.current_frame_mut();
                        frame.ip = (frame.ip as i32 + offset) as usize;
                    }
                    // If falsy, IP already advanced past jump instruction
                }

                Opcode::Return => {
                    // Return the top value and halt execution
                    return self.pop();
                }

                Opcode::Closure => {
                    // Read function index from constants
                    let func_index_in_constants = self.read_u16_operand()?;
                    let chunk = self.current_chunk();

                    if func_index_in_constants as usize >= chunk.constants.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Invalid constant index: {}", func_index_in_constants),
                            source_id: chunk.source_id.to_string(),
                        });
                    }

                    let func_value = chunk.constants[func_index_in_constants as usize];
                    let func_index = if let Value::Function(idx) = func_value {
                        idx
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "Expected function in constants, got {:?}",
                                func_value
                            ),
                            source_id: chunk.source_id.to_string(),
                        });
                    };

                    // Read upvalue count from bytecode
                    let frame = self.current_frame();
                    if frame.ip >= chunk.count() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Invalid bytecode - missing upvalue count".to_string(),
                            source_id: chunk.source_id.to_string(),
                        });
                    }
                    let upvalue_count = chunk.code[frame.ip] as usize;
                    self.current_frame_mut().ip += 1;

                    // Collect upvalue indices
                    let mut upvalue_indices = Vec::with_capacity(upvalue_count);

                    for _ in 0..upvalue_count {
                        let frame = self.current_frame();
                        let chunk = self.current_chunk();

                        // Read is_local flag
                        if frame.ip >= chunk.count() {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Invalid bytecode - missing upvalue is_local flag"
                                    .to_string(),
                                source_id: chunk.source_id.to_string(),
                            });
                        }
                        let is_local = chunk.code[frame.ip] != 0;
                        self.current_frame_mut().ip += 1;

                        // Read index (u16)
                        let frame = self.current_frame();
                        let chunk = self.current_chunk();
                        if frame.ip + 1 >= chunk.count() {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Invalid bytecode - missing upvalue index".to_string(),
                                source_id: chunk.source_id.to_string(),
                            });
                        }
                        let index_bytes = [chunk.code[frame.ip], chunk.code[frame.ip + 1]];
                        let index = u16::from_le_bytes(index_bytes) as usize;
                        self.current_frame_mut().ip += 2;

                        // Capture or copy upvalue
                        let upvalue_index = if is_local {
                            // Capture from stack
                            let frame = self.current_frame();
                            let stack_location = frame.stack_base + index;
                            self.capture_upvalue(stack_location)
                        } else {
                            // Copy from current closure's upvalues
                            let frame = self.current_frame();
                            let current_closure = self.memory_manager.load_closure(frame.closure);
                            if index >= current_closure.upvalues.len() {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "Invalid upvalue index {} (closure has {} upvalues)",
                                        index,
                                        current_closure.upvalues.len()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                            current_closure.upvalues[index]
                        };

                        upvalue_indices.push(upvalue_index);
                    }

                    // Create closure
                    let closure_allocation = self
                        .memory_manager
                        .allocate_closure(func_index, upvalue_indices);
                    self.push(Value::Closure(closure_allocation.index))?;

                    if closure_allocation.should_garbage_collect {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[VirtualMachine] Running GC at PC={} (Closure allocation)",
                                self.current_frame().ip
                            );
                        }
                        self.run_garbage_collection();
                    }
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

    /// Compare two values for equality according to Jsonnet semantics
    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            // Same type comparisons
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b, // compares only the keys and so can be quick
            (Value::Function(a), Value::Function(b)) => a == b, // compare function indices
            (Value::Closure(a), Value::Closure(b)) => a == b, // compare closure indices

            // Different types are never equal
            _ => false,
        }
    }

    /// Convert a VM Value to serde_json::Value for JSON output
    fn value_to_json(
        &self,
        value: &Value,
        visited: &mut std::collections::HashSet<ObjectIndex>,
    ) -> Result<serde_json::Value, RuntimeError> {
        match value {
            Value::Null => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Number(n) => serde_json::Number::from_f64(*n)
                .map(serde_json::Value::Number)
                .ok_or_else(|| RuntimeError {
                    span: 0..0,
                    message: "Invalid number for JSON conversion".to_string(),
                    source_id: "serialization".to_string(),
                }),
            Value::String(s) => Ok(serde_json::Value::String(
                self.memory_manager.load_string(*s).to_owned(),
            )),
            Value::Object(object_key) => {
                // Check for circular references
                if visited.contains(object_key) {
                    return Err(RuntimeError {
                        span: 0..0,
                        message: "Circular reference detected in object".to_string(),
                        source_id: "serialization".to_string(),
                    });
                }

                visited.insert(*object_key);

                let object = self.memory_manager.load_object(*object_key);
                let mut json_object = serde_json::Map::new();

                for (key, value) in &object.properties {
                    let json_value = self.value_to_json(value, visited)?;
                    json_object
                        .insert(self.memory_manager.load_string(*key).to_owned(), json_value);
                }

                visited.remove(object_key); // Remove after processing
                Ok(serde_json::Value::Object(json_object))
            }
            Value::Array(array_key) => {
                let array = self.memory_manager.load_array(*array_key);
                let mut json_array = Vec::new();

                for element in &array.elements {
                    let json_value = self.value_to_json(element, visited)?;
                    json_array.push(json_value);
                }

                Ok(serde_json::Value::Array(json_array))
            }
            Value::Function(_) => Err(RuntimeError {
                span: 0..0,
                message: "Cannot serialize function to JSON".to_string(),
                source_id: "serialization".to_string(),
            }),
            Value::Closure(_) => Err(RuntimeError {
                span: 0..0,
                message: "Cannot serialize closure to JSON".to_string(),
                source_id: "serialization".to_string(),
            }),
        }
    }

    fn run_garbage_collection(&mut self) {
        let mut roots = Vec::from(self.stack.clone());

        // Add all active frames' closures as roots
        for i in 0..self.frame_count {
            roots.push(Value::Closure(self.frames[i].closure));
        }

        self.memory_manager.run_garbage_collect(roots);
    }

    fn is_truthy(&self, value: Value) -> bool {
        match value {
            Value::Null => false,
            Value::Boolean(b) => b,
            Value::Number(n) => n > 0.0,
            Value::String(s) => self.memory_manager.load_string(s) != "",
            Value::Object(x) => self.memory_manager.load_object(x).len() > 0,
            Value::Array(x) => self.memory_manager.load_array(x).len() > 0,
            Value::Function(_) => true, // Functions are truthy
            Value::Closure(_) => true,  // Closures are truthy
        }
    }

    fn to_number(&self, value: Value) -> Result<f64, RuntimeError> {
        match value {
            Value::Number(n) => Ok(n),
            Value::String(key) => {
                // @TODO: this is weird. should refactor to map_err or someother
                match self.memory_manager.load_string(key).parse::<f64>() {
                    Ok(n) => Ok(n),
                    Err(e) => Err(RuntimeError {
                        span: self.get_current_span(),
                        message: format!("Failed to parse string {} to f64", e),
                        source_id: self.current_chunk().source_id.to_string(),
                    }),
                }
            }
            _ => Err(RuntimeError {
                span: self.get_current_span(),
                message: format!("Cannot convert {:?} to f64", value),
                source_id: self.current_chunk().source_id.to_string(),
            }),
        }
    }

    fn to_integer(&self, value: Value) -> Result<i64, RuntimeError> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() {
            Err(RuntimeError {
                span: self.get_current_span(),
                message: "Cannot convert NaN or Infinity to integer".to_string(),
                source_id: self.current_chunk().source_id.to_string(),
            })
        } else {
            Ok(n as i64)
        }
    }
}

/// Main execution function - entry point for running Jsonnet bytecode
pub fn execute(
    chunk: Chunk,
    memory_manager: MemoryManager,
) -> Result<serde_json::Value, RuntimeError> {
    let mut vm = VirtualMachine::new(chunk, memory_manager);

    let value = vm.interpret()?;

    // Convert VM value to JSON value with circular reference detection
    let mut visited = std::collections::HashSet::new();
    vm.value_to_json(&value, &mut visited)
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
        let chunk = create_test_chunk();
        let memory_manager = MemoryManager::new();
        let vm = VirtualMachine::new(chunk, memory_manager);

        assert!(!vm.is_truthy(Value::Null));
        assert!(!vm.is_truthy(Value::Boolean(false)));
        assert!(vm.is_truthy(Value::Boolean(true)));
        assert!(!vm.is_truthy(Value::Number(0.0)));
        assert!(vm.is_truthy(Value::Number(1.0)));
        assert!(!vm.is_truthy(Value::Number(-1.0)));
        assert!(vm.is_truthy(Value::Number(0.1)));
    }

    #[test]
    fn test_load_null() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_load_true() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_load_false() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadFalse, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_load_const() {
        let mut chunk = create_test_chunk();
        let const_index = chunk.add_constant(Value::Number(42.0));
        chunk.write_opcode_u16(Opcode::LoadConst, const_index as u16, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_add() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(10.0));
        let idx2 = chunk.add_constant(Value::Number(5.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(15.0));
    }

    #[test]
    fn test_subtract() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(10.0));
        let idx2 = chunk.add_constant(Value::Number(3.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Sub, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_multiply() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(6.0));
        let idx2 = chunk.add_constant(Value::Number(7.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Mul, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_divide() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(15.0));
        let idx2 = chunk.add_constant(Value::Number(3.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Div, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_divide_by_zero() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(10.0));
        let idx2 = chunk.add_constant(Value::Number(0.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Div, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Division by zero"));
    }

    #[test]
    fn test_comparison_lt() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(5.0));
        let idx2 = chunk.add_constant(Value::Number(10.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Lt, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_bitwise_shl() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(8.0));
        let idx2 = chunk.add_constant(Value::Number(2.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Shl, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(32.0)); // 8 << 2 = 32
    }

    #[test]
    fn test_neg() {
        let mut chunk = create_test_chunk();
        let idx = chunk.add_constant(Value::Number(42.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx as u16, 0..5);
        chunk.write_opcode(Opcode::Neg, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-42.0));
    }

    #[test]
    fn test_not() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Not, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_pop() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(1.0));
        let idx2 = chunk.add_constant(Value::Number(2.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Pop, 10..15); // Pop 2.0
        chunk.write_opcode(Opcode::Return, 15..20); // Return 1.0

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_dup() {
        let mut chunk = create_test_chunk();
        let idx = chunk.add_constant(Value::Number(42.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx as u16, 0..5);
        chunk.write_opcode(Opcode::Dup, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15); // 42 + 42
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(84.0));
    }

    #[test]
    fn test_swap() {
        let mut chunk = create_test_chunk();
        let idx1 = chunk.add_constant(Value::Number(10.0));
        let idx2 = chunk.add_constant(Value::Number(3.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx1 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx2 as u16, 5..10);
        chunk.write_opcode(Opcode::Swap, 10..15); // Now stack is [3, 10]
        chunk.write_opcode(Opcode::Sub, 15..20); // 3 - 10 = -7
        chunk.write_opcode(Opcode::Return, 20..25);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-7.0));
    }

    #[test]
    fn test_stack_underflow() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::Add, 0..5); // Try to add with empty stack
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Stack underflow"));
    }

    #[test]
    fn test_invalid_constant_index() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode_u16(Opcode::LoadConst, 999, 0..5); // Invalid index
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("Invalid constant index")
        );
    }

    #[test]
    fn test_missing_return() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        // Missing Return opcode

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("missing Return instruction")
        );
    }

    #[test]
    fn test_unimplemented_opcode() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::CreateObjectComp, 0..5); // Unimplemented opcode
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Unimplemented opcode"));
    }

    #[test]
    fn test_complex_expression() {
        // Test: (10 + 5) * 2 - 3 = 27
        let mut chunk = create_test_chunk();
        let idx_10 = chunk.add_constant(Value::Number(10.0));
        let idx_5 = chunk.add_constant(Value::Number(5.0));
        let idx_2 = chunk.add_constant(Value::Number(2.0));
        let idx_3 = chunk.add_constant(Value::Number(3.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx_10 as u16, 0..5);
        chunk.write_opcode_u16(Opcode::LoadConst, idx_5 as u16, 5..10);
        chunk.write_opcode(Opcode::Add, 10..15); // 15
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 15..20);
        chunk.write_opcode(Opcode::Mul, 20..25); // 30
        chunk.write_opcode_u16(Opcode::LoadConst, idx_3 as u16, 25..30);
        chunk.write_opcode(Opcode::Sub, 30..35); // 27
        chunk.write_opcode(Opcode::Return, 35..40);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(27.0));
    }

    #[test]
    fn test_error_string_execution() {
        let mut chunk = create_test_chunk();
        let mut memory_manager = MemoryManager::new();

        // Allocate the string in memory manager first
        let msg_string = memory_manager.allocate_string("test message");
        let error_msg = chunk.add_constant(Value::String(msg_string.index));

        chunk.write_opcode_u16(Opcode::LoadConst, error_msg as u16, 0..5);
        chunk.write_opcode(Opcode::Error, 5..10);

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.message, "\"test message\""); // JSON string representation
        assert_eq!(error.span, 5..10); // Error keyword span
    }

    #[test]
    fn test_error_number_execution() {
        let mut chunk = create_test_chunk();
        let error_val = chunk.add_constant(Value::Number(42.0));

        chunk.write_opcode_u16(Opcode::LoadConst, error_val as u16, 0..5);
        chunk.write_opcode(Opcode::Error, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.message, "42.0"); // JSON number representation
        assert_eq!(error.span, 5..10); // Error keyword span
    }

    #[test]
    fn test_error_boolean_execution() {
        let mut chunk = create_test_chunk();
        let error_val = chunk.add_constant(Value::Boolean(true));

        chunk.write_opcode_u16(Opcode::LoadConst, error_val as u16, 0..5);
        chunk.write_opcode(Opcode::Error, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.message, "true"); // JSON boolean representation
        assert_eq!(error.span, 5..10); // Error keyword span
    }

    #[test]
    fn test_jump_opcode() {
        let mut chunk = create_test_chunk();

        // LoadConst 1, Jump, LoadConst 2, Return, LoadConst 3, Return
        let idx_1 = chunk.add_constant(Value::Number(1.0));
        let idx_2 = chunk.add_constant(Value::Number(2.0));
        let idx_3 = chunk.add_constant(Value::Number(3.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx_1 as u16, 0..5); // [0-2]: 3 bytes
        chunk.write_opcode_i32(Opcode::Jump, 4, 5..10); // [3-7]: 5 bytes, jump +4 to skip next 4 bytes
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 10..15); // [8-10]: 3 bytes (skipped)
        chunk.write_opcode(Opcode::Return, 15..20); // [11]: 1 byte (skipped)
        // Jump target is at position 8+4=12
        chunk.write_opcode_u16(Opcode::LoadConst, idx_3 as u16, 20..25); // [12-14]: jumped to
        chunk.write_opcode(Opcode::Return, 25..30); // [15]

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // Should load 1, jump over loading 2, load 3, return 3
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_jump_if_false_truthy() {
        let mut chunk = create_test_chunk();

        let idx_42 = chunk.add_constant(Value::Number(42.0));
        let idx_99 = chunk.add_constant(Value::Number(99.0));

        chunk.write_opcode(Opcode::LoadTrue, 0..5); // [0]: 1 byte, condition = true
        chunk.write_opcode_i32(Opcode::JumpIfFalse, 4, 5..10); // [1-5]: 5 bytes, jump +4 to skip next 4 bytes
        chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 10..15); // [6-8]: 3 bytes, load 42 (executed)
        chunk.write_opcode(Opcode::Return, 15..20); // [9]: 1 byte, return
        chunk.write_opcode_u16(Opcode::LoadConst, idx_99 as u16, 20..25); // [10-12]: 3 bytes, load 99 (skipped)
        chunk.write_opcode(Opcode::Return, 25..30); // [13]: 1 byte, return

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // true condition -> don't jump -> execute 42
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_jump_if_false_falsy() {
        let mut chunk = create_test_chunk();

        let idx_42 = chunk.add_constant(Value::Number(42.0));
        let idx_99 = chunk.add_constant(Value::Number(99.0));

        chunk.write_opcode(Opcode::LoadFalse, 0..5); // [0]: 1 byte, condition = false
        chunk.write_opcode_i32(Opcode::JumpIfFalse, 4, 5..10); // [1-5]: 5 bytes, jump +4 if false
        chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 10..15); // [6-8]: 3 bytes, load 42 (skipped)
        chunk.write_opcode(Opcode::Return, 15..20); // [9]: 1 byte, return (skipped)
        chunk.write_opcode_u16(Opcode::LoadConst, idx_99 as u16, 20..25); // [10-12]: 3 bytes, load 99 (executed after jump)
        chunk.write_opcode(Opcode::Return, 25..30); // [13]: 1 byte, return

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // false condition -> jump -> execute 99
        assert_eq!(result, Value::Number(99.0));
    }

    #[test]
    fn test_jump_if_true_truthy() {
        let mut chunk = create_test_chunk();

        let idx_42 = chunk.add_constant(Value::Number(42.0));
        let idx_99 = chunk.add_constant(Value::Number(99.0));

        chunk.write_opcode(Opcode::LoadTrue, 0..5); // [0]: 1 byte, condition = true
        chunk.write_opcode_i32(Opcode::JumpIfTrue, 4, 5..10); // [1-5]: 5 bytes, jump +4 if true
        chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 10..15); // [6-8]: 3 bytes, load 42 (skipped)
        chunk.write_opcode(Opcode::Return, 15..20); // [9]: 1 byte, return (skipped)
        chunk.write_opcode_u16(Opcode::LoadConst, idx_99 as u16, 20..25); // [10-12]: 3 bytes, load 99 (executed after jump)
        chunk.write_opcode(Opcode::Return, 25..30); // [13]: 1 byte, return

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // true condition -> jump -> execute 99
        assert_eq!(result, Value::Number(99.0));
    }

    #[test]
    fn test_jump_if_true_falsy() {
        let mut chunk = create_test_chunk();

        let idx_42 = chunk.add_constant(Value::Number(42.0));
        let idx_99 = chunk.add_constant(Value::Number(99.0));

        chunk.write_opcode(Opcode::LoadFalse, 0..5); // [0]: 1 byte, condition = false
        chunk.write_opcode_i32(Opcode::JumpIfTrue, 4, 5..10); // [1-5]: 5 bytes, don't jump if false
        chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 10..15); // [6-8]: 3 bytes, load 42 (executed)
        chunk.write_opcode(Opcode::Return, 15..20); // [9]: 1 byte, return
        chunk.write_opcode_u16(Opcode::LoadConst, idx_99 as u16, 20..25); // [10-12]: 3 bytes, load 99 (skipped)
        chunk.write_opcode(Opcode::Return, 25..30); // [13]: 1 byte, return

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // false condition -> don't jump -> execute 42
        assert_eq!(result, Value::Number(42.0));
    }

    // LoadVar opcode tests

    #[test]
    fn test_loadvar_opcode() {
        let mut chunk = create_test_chunk();

        // Push two values on stack
        let idx_1 = chunk.add_constant(Value::Number(10.0));
        let idx_2 = chunk.add_constant(Value::Number(20.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx_1 as u16, 0..5); // stack[0] = 10
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 5..10); // stack[1] = 20
        chunk.write_opcode_u16(Opcode::LoadVar, 0, 10..15); // Load from slot 0
        chunk.write_opcode(Opcode::Return, 15..20);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        // Should return value from slot 0 (10.0)
        assert_eq!(result, Value::Number(10.0));
    }

    #[test]
    fn test_loadvar_multiple_slots() {
        let mut chunk = create_test_chunk();

        let idx_1 = chunk.add_constant(Value::Number(10.0));
        let idx_2 = chunk.add_constant(Value::Number(20.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx_1 as u16, 0..5); // stack[0] = 10
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 5..10); // stack[1] = 20
        chunk.write_opcode_u16(Opcode::LoadVar, 0, 10..15); // Load slot 0 (10)
        chunk.write_opcode_u16(Opcode::LoadVar, 1, 15..20); // Load slot 1 (20)
        chunk.write_opcode(Opcode::Add, 20..25); // 10 + 20
        chunk.write_opcode(Opcode::Return, 25..30);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(30.0));
    }

    #[test]
    fn test_loadvar_invalid_slot() {
        let mut chunk = create_test_chunk();

        chunk.write_opcode_u16(Opcode::LoadVar, 99, 0..5); // Invalid slot
        chunk.write_opcode(Opcode::Return, 5..10);

        let memory_manager = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Invalid stack slot"));
    }
}
