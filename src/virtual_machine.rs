use std::ops::Range;
use chunk::{Chunk, Opcode, Value, RuntimeError, JsonnetObject};
use string_pool::{InternedString, StringPool};
use slotmap::{SlotMap, DefaultKey};

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
    /// Object storage using SlotMap for stable references
    objects: SlotMap<DefaultKey, JsonnetObject>,
    /// Bytes allocated for object storage (for GC threshold tracking)
    objects_allocated_bytes: usize,
    /// Threshold for triggering object garbage collection
    next_object_garbage_collection: usize,
    /// String pool for string interning and GC
    string_pool: StringPool,
}

impl<'a> VirtualMachine<'a> {
    /// Create a new virtual machine with the given starting chunk and string pool
    pub fn new(chunk: Chunk<'a>, string_pool: StringPool) -> Self {
        let mut vm = Self {
            chunks: Vec::new(),
            current_chunk: 0,
            program_counter: 0,
            stack: Vec::with_capacity(1024),
            objects: SlotMap::new(),
            objects_allocated_bytes: 0,
            next_object_garbage_collection: 1024 * 1024, // Initial 1MB threshold
            string_pool,
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

    /// Allocate a new object in the SlotMap and return its key
    pub fn allocate_object(&mut self, object: JsonnetObject) -> DefaultKey {
        let object_size = object.allocated_bytes();
        self.objects_allocated_bytes += object_size;
        self.objects.insert(object)
    }

    /// GC-aware object allocation that triggers collection if needed
    pub fn allocate_object_with_gc(&mut self, object: JsonnetObject, string_pool: &mut StringPool, string_roots: Vec<InternedString>) -> DefaultKey {
        // Check if GC should run before allocation
        if self.should_collect() || string_pool.should_collect() {
            // Collect from object roots (get object keys from stack and all objects)
            let mut object_roots = Vec::new();
            
            // Add objects from the stack as roots
            for value in &self.stack {
                if let Value::Object(key) = value {
                    object_roots.push(*key);
                }
            }
            
            // Add all existing objects as roots (simplified approach)
            for (key, _) in &self.objects {
                object_roots.push(key);
            }
            
            // Collect strings first (may free object references)  
            string_pool.collect_garbage(string_roots);
            
            // Then collect objects
            self.collect_objects(&object_roots);
        }
        
        // Perform the allocation
        let object_size = object.allocated_bytes();
        self.objects_allocated_bytes += object_size;
        self.objects.insert(object)
    }

    /// Get an object from the SlotMap by its key
    pub fn get_object(&self, key: DefaultKey) -> Option<&JsonnetObject> {
        self.objects.get(key)
    }

    /// Get a mutable reference to an object from the SlotMap by its key
    pub fn get_object_muts(&mut self, key: DefaultKey) -> Option<&mut JsonnetObject> {
        self.objects.get_mut(key)
    }

    /// Check if an object key is valid
    pub fn is_valid_object(&self, key: DefaultKey) -> bool {
        self.objects.contains_key(key)
    }

    /// Create an empty object and return its key
    pub fn create_empty_object(&mut self) -> DefaultKey {
        let object = JsonnetObject::new();
        // TODO: Use GC-aware allocation - requires StringPool refactoring
        self.allocate_object(object)
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

                    let constant = chunk.constants[index as usize].clone();
                    self.push(constant)?;
                }

                // Binary arithmetic operations
                Opcode::Add => {
                    let b = self.pop()?;
                    let a = self.pop()?;

                    // Check for different addition types
                    match (&a, &b) {
                        // Object merging (according to Jsonnet spec)
                        (Value::Object(left_key), Value::Object(right_key)) => {
                            if let (Some(left_object), Some(right_object)) = (self.get_object(*left_key), self.get_object(*right_key)) {
                                // Create merged properties starting with left object
                                let mut merged_properties = left_object.properties.clone();

                                // Override/add properties from right object
                                for (key, value) in &right_object.properties {
                                    merged_properties.insert(*key, value.clone());
                                }

                                let merged_object = JsonnetObject::with_properties(merged_properties);
                                // TODO: Use GC-aware allocation - requires StringPool refactoring
                                let merged_key = self.allocate_object(merged_object);
                                self.push(Value::Object(merged_key))?;
                            } else {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "Invalid object reference in merge".to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        }
                        (Value::Object(_), _) | (_, Value::Object(_)) => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Must concatenate objects with other objects".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            })
                        }
                        // String concatenation if either operand is a string
                        (Value::String(_), _) | (_, Value::String(_)) => {
                            let a_str = match &a {
                                Value::String(s) => s.as_str().to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => unreachable!(),
                            };
                            let b_str = match &b {
                                Value::String(s) => s.as_str().to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => unreachable!(),
                            };
                            let result_str = format!("{}{}", a_str, b_str);
                            let roots = self.get_string_roots();
                            let interned = self.string_pool.intern_with_gc(&result_str, roots);
                            self.push(Value::String(interned))?;
                        }
                        // Numeric addition for all other cases
                        _ => {
                            let result = a.to_number(self.get_current_span(), self.current_chunk().source_id)? + b.to_number(self.get_current_span(), self.current_chunk().source_id)?;
                            self.push(Value::Number(result))?;
                        }
                    }
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
                    let result = match (&a, &b) {
                        (Value::String(s1), Value::String(s2)) => {
                            let result_str = format!("{}{}", s1.as_str(), s2.as_str());
                            let roots = self.get_string_roots();
                            let interned = self.string_pool.intern_with_gc(&result_str, roots);
                            Value::String(interned)
                        }
                        _ => {
                            // This should not happen if compiler type tracking is correct
                            // Fall back to safe conversion
                            let a_str = match &a {
                                Value::String(s) => s.as_str().to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                            };
                            let b_str = match &b {
                                Value::String(s) => s.as_str().to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                            };
                            let result_str = format!("{}{}", a_str, b_str);
                            let roots = self.get_string_roots();
                            let interned = self.string_pool.intern_with_gc(&result_str, roots);
                            Value::String(interned)
                        }
                    };

                    self.push(result)?;
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

                    let object = JsonnetObject::with_properties(properties);
                    // TODO: Use GC-aware allocation - requires StringPool refactoring
                    let object_key = self.allocate_object(object);
                    self.push(Value::Object(object_key))?;
                }

                Opcode::ObjectIndex => {
                    let field_name = self.pop()?;  // Property name to access
                    let object_value = self.pop()?;  // Object to index into

                    // Ensure we have an object
                    if let Value::Object(object_key) = object_value {
                        // Ensure field name is a string
                        if let Value::String(field_key) = field_name {
                            if let Some(object) = self.get_object(object_key) {
                                if let Some(value) = object.get(&field_key) {
                                    self.push(value.clone())?;
                                } else {
                                    // Property doesn't exist, push null
                                    self.push(Value::Null)?;
                                }
                            } else {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "Invalid object reference".to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        } else {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("Object index must be a string, got {:?}", field_name),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Cannot index into non-object value: {:?}", object_value),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    self.advance_pc();
                }

                Opcode::ObjectMerge => {
                    let right_value = self.pop()?;  // Right-hand side object
                    let left_value = self.pop()?;   // Left-hand side object

                    // Ensure both values are objects
                    if let (Value::Object(left_key), Value::Object(right_key)) = (left_value, right_value) {
                        if let (Some(left_object), Some(right_object)) = (self.get_object(left_key), self.get_object(right_key)) {
                            // Create merged properties starting with left object
                            let mut merged_properties = left_object.properties.clone();

                            // Override/add properties from right object
                            for (key, value) in &right_object.properties {
                                merged_properties.insert(*key, value.clone());
                            }

                            let merged_object = JsonnetObject::with_properties(merged_properties);
                            // TODO: Use GC-aware allocation - requires StringPool refactoring
                            let merged_key = self.allocate_object(merged_object);
                            self.push(Value::Object(merged_key))?;
                        } else {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Invalid object reference in merge".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
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

    /// Compare two values for equality according to Jsonnet semantics
    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            // Same type comparisons
            (Value::Null, Value::Null) => true,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a.ptr_eq(*b), // O(1) pointer comparison!

            // Different types are never equal
            _ => false,
        }
    }

    /// Collect all GC roots from the VM for garbage collection
    pub fn collect_gc_roots(&self) -> (Vec<InternedString>, Vec<DefaultKey>) {
        let mut string_roots = Vec::new();
        let mut object_roots = Vec::new();

        // Collect from value stack
        for value in &self.stack {
            match value {
                Value::String(interned_string) => string_roots.push(*interned_string),
                Value::Object(object_key) => object_roots.push(*object_key),
                _ => {}
            }
        }

        // Collect from constants in all loaded chunks
        for chunk in &self.chunks {
            for constant in &chunk.constants {
                match constant {
                    Value::String(interned_string) => string_roots.push(*interned_string),
                    Value::Object(object_key) => object_roots.push(*object_key),
                    _ => {}
                }
            }
        }

        // Collect strings from object properties
        self.collect_strings_from_objects(&object_roots, &mut string_roots);

        // Future: Collect from other VM roots:
        // - Local variables
        // - Global variables
        // - Call frames

        (string_roots, object_roots)
    }

    /// Recursively collect string roots from objects and their properties
    fn collect_strings_from_objects(&self, object_keys: &[DefaultKey], string_roots: &mut Vec<InternedString>) {
        let mut visited_objects = std::collections::HashSet::new();
        let mut objects_to_visit = object_keys.to_vec();

        while let Some(object_key) = objects_to_visit.pop() {
            if visited_objects.contains(&object_key) {
                continue;
            }
            visited_objects.insert(object_key);

            if let Some(object) = self.get_object(object_key) {
                for (property_key, property_value) in &object.properties {
                    // Add property key (which is an InternedString)
                    string_roots.push(*property_key);

                    // If property value is a string, add it
                    // If property value is an object, add it to visit queue
                    match property_value {
                        Value::String(interned_string) => string_roots.push(*interned_string),
                        Value::Object(nested_object_key) => {
                            if !visited_objects.contains(nested_object_key) {
                                objects_to_visit.push(*nested_object_key);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Get string roots for GC-aware string allocation
    fn get_string_roots(&self) -> Vec<InternedString> {
        let mut roots = Vec::new();
        
        // Add strings from the stack as roots
        for value in &self.stack {
            if let Value::String(s) = value {
                roots.push(*s);
            }
        }
        
        // Add strings from object properties as roots
        for (_, object) in &self.objects {
            for (key, value) in &object.properties {
                roots.push(*key); // Property key is an InternedString
                if let Value::String(s) = value {
                    roots.push(*s); // Property value if it's a string
                }
            }
        }
        
        roots
    }

    /// Check if object garbage collection should be triggered
    fn should_collect(&self) -> bool {
        #[cfg(feature = "stress_gc")]
        {
            eprintln!("[GC] Stress GC enabled - triggering object collection ({} objects, {} bytes)",
                     self.objects.len(), self.objects_allocated_bytes);
            return true;
        }

        let should_collect = self.objects_allocated_bytes >= self.next_object_garbage_collection;
        if should_collect {
            eprintln!("[GC] Object threshold exceeded - triggering object collection ({} bytes >= {} bytes, {} objects)",
                     self.objects_allocated_bytes, self.next_object_garbage_collection, self.objects.len());
        }
        should_collect
    }

    /// Perform object garbage collection with threshold updating
    fn collect_objects(&mut self, roots: &[DefaultKey]) {
        let initial_count = self.objects.len();
        let initial_bytes = self.objects_allocated_bytes;

        eprintln!("[VM GC] Mark phase: Processing {} roots from {} total objects ({} bytes)",
                 roots.len(), initial_count, initial_bytes);

        // Simple mark-and-sweep for objects
        let mut reachable_objects = std::collections::HashSet::new();
        let mut objects_to_visit = roots.to_vec();

        // Mark phase: find all reachable objects
        while let Some(object_key) = objects_to_visit.pop() {
            if reachable_objects.contains(&object_key) {
                continue;
            }

            if let Some(object) = self.get_object(object_key) {
                reachable_objects.insert(object_key);

                // Add nested objects to visit queue
                for (_, value) in &object.properties {
                    if let Value::Object(nested_key) = value {
                        if !reachable_objects.contains(nested_key) {
                            objects_to_visit.push(*nested_key);
                        }
                    }
                }
            }
        }

        // Sweep phase: remove unreachable objects and update byte accounting
        let all_keys: Vec<DefaultKey> = self.objects.keys().collect();
        for key in all_keys {
            if !reachable_objects.contains(&key) {
                if let Some(object) = self.objects.get(key) {
                    let object_size = object.allocated_bytes();
                    self.objects_allocated_bytes -= object_size;
                }
                self.objects.remove(key);
            }
        }

        let final_count = self.objects.len();
        let final_bytes = self.objects_allocated_bytes;
        let old_threshold = self.next_object_garbage_collection;

        // Update threshold: current size * 2, minimum 1MB
        self.next_object_garbage_collection = std::cmp::max(
            self.objects_allocated_bytes * 2,
            1024 * 1024 // Minimum 1MB threshold
        );

        eprintln!("[VM GC] Complete: {} -> {} objects, {} -> {} bytes, threshold: {} -> {} bytes",
                 initial_count, final_count, initial_bytes, final_bytes,
                 old_threshold, self.next_object_garbage_collection);
    }

    /// Get object allocation statistics
    pub fn object_stats(&self) -> (usize, usize, usize) {
        (
            self.objects_allocated_bytes,
            self.next_object_garbage_collection,
            self.objects.len()
        )
    }

    /// Convert a VM Value to serde_json::Value for JSON output
    fn value_to_json(&self, value: &Value, visited: &mut std::collections::HashSet<DefaultKey>) -> Result<serde_json::Value, RuntimeError> {
        match value {
            Value::Null => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Number(n) => {
                serde_json::Number::from_f64(*n)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| RuntimeError {
                        span: 0..0,
                        message: "Invalid number for JSON conversion".to_string(),
                        source_id: "serialization".to_string(),
                    })
            },
            Value::String(s) => Ok(serde_json::Value::String(s.as_str().to_owned())),
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

                if let Some(object) = self.get_object(*object_key) {
                    let mut json_object = serde_json::Map::new();

                    for (key, value) in &object.properties {
                        let json_value = self.value_to_json(value, visited)?;
                        json_object.insert(key.as_str().to_owned(), json_value);
                    }

                    visited.remove(object_key); // Remove after processing
                    Ok(serde_json::Value::Object(json_object))
                } else {
                    Err(RuntimeError {
                        span: 0..0,
                        message: "Invalid object reference during JSON conversion".to_string(),
                        source_id: "serialization".to_string(),
                    })
                }
            }
        }
    }
}

impl<'a> Drop for VirtualMachine<'a> {
    fn drop(&mut self) {
        let object_count = self.objects.len();
        let allocated_bytes = self.objects_allocated_bytes;
        
        eprintln!("[VirtualMachine] Deallocating {} objects ({} bytes) on drop", 
                 object_count, allocated_bytes);
        
        // Explicitly clear to break any potential circular references
        self.objects.clear();
        
        eprintln!("[VirtualMachine] Object cleanup complete");
        // StringPool will handle its own cleanup via its Drop impl
    }
}

/// Main execution function - entry point for running Jsonnet bytecode
pub fn execute(chunk: Chunk, string_pool: StringPool) -> Result<serde_json::Value, RuntimeError> {
    let mut vm = VirtualMachine::new(chunk, string_pool);

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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Null);
    }

    #[test]
    fn test_load_true() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_load_false() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadFalse, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_load_const() {
        let mut chunk = create_test_chunk();
        let const_index = chunk.add_constant(Value::Number(42.0));
        chunk.write_opcode_u16(Opcode::LoadConst, const_index as u16, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_neg() {
        let mut chunk = create_test_chunk();
        let idx = chunk.add_constant(Value::Number(42.0));

        chunk.write_opcode_u16(Opcode::LoadConst, idx as u16, 0..5);
        chunk.write_opcode(Opcode::Neg, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-42.0));
    }

    #[test]
    fn test_not() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadTrue, 0..5);
        chunk.write_opcode(Opcode::Not, 5..10);
        chunk.write_opcode(Opcode::Return, 10..15);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(-7.0));
    }

    #[test]
    fn test_stack_underflow() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::Add, 0..5); // Try to add with empty stack
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Stack underflow"));
    }

    #[test]
    fn test_invalid_constant_index() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode_u16(Opcode::LoadConst, 999, 0..5); // Invalid index
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("Invalid constant index"));
    }

    #[test]
    fn test_missing_return() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        // Missing Return opcode

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("missing Return instruction"));
    }

    #[test]
    fn test_unimplemented_opcode() {
        let mut chunk = create_test_chunk();
        chunk.write_opcode(Opcode::CreateArray, 0..5); // Unimplemented opcode
        chunk.write_opcode(Opcode::Return, 5..10);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
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
        chunk.write_opcode(Opcode::Add, 10..15);  // 15
        chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 15..20);
        chunk.write_opcode(Opcode::Mul, 20..25);  // 30
        chunk.write_opcode_u16(Opcode::LoadConst, idx_3 as u16, 25..30);
        chunk.write_opcode(Opcode::Sub, 30..35);  // 27
        chunk.write_opcode(Opcode::Return, 35..40);

        let string_pool = StringPool::new();
        let mut vm = VirtualMachine::new(chunk, string_pool);
        let result = vm.interpret().unwrap();

        assert_eq!(result, Value::Number(27.0));
    }
}
