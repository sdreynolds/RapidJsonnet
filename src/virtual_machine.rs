use chunk::{
    Chunk, ClosureIndex, FieldVisibility, I32_SIZE_BYTES, OPCODE_SIZE_BYTES, ObjectIndex, Opcode,
    OwnedChunk, RuntimeError, StringIndex, UpvalueIndex, Value,
};
use compiler;
use memory_manager::{MemoryManager, ObjectField};
use scanner;
use std::ops::Range;

use native::{self, call_native};

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
    /// The current object (self) for this frame
    pub self_obj: Option<ObjectIndex>,
    /// The current super object for this frame
    pub super_obj: Option<ObjectIndex>,
}

impl CallFrame {
    /// Create a new call frame
    pub fn new(
        closure: ClosureIndex,
        ip: usize,
        stack_base: usize,
        self_obj: Option<ObjectIndex>,
        super_obj: Option<ObjectIndex>,
    ) -> Self {
        Self {
            closure,
            ip,
            stack_base,
            self_obj,
            super_obj,
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
    /// Start IP of the instruction currently being executed
    instruction_start_ip: usize,
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
        let initial_frame = CallFrame::new(closure_result.index, 0, 0, None, None);

        let mut frames = Vec::with_capacity(MAX_FRAMES);
        frames.push(initial_frame);

        Self {
            frames,
            frame_count: 1,
            open_upvalues: None,
            stack: Vec::with_capacity(256),
            memory_manager,
            instruction_start_ip: 0,
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

    /// Pop a value and immediately force it if it's an Import Thunk
    fn pop_forced(&mut self) -> Result<Value, RuntimeError> {
        let val = self.pop()?;
        self.force_value(val)
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
            .get_span(self.instruction_start_ip)
            .cloned()
            .unwrap_or(0..0)
    }

    /// Read a u8 operand from the current position and advance PC
    fn read_u8_operand(&mut self) -> Result<u8, RuntimeError> {
        let frame = self.current_frame();
        let chunk = self.current_chunk();
        let operand = chunk.read_u8(frame.ip + 1).ok_or_else(|| RuntimeError {
            span: self.get_current_span(),
            message: "Invalid bytecode - missing operand".to_string(),
            source_id: chunk.source_id.to_string(),
        })?;

        self.current_frame_mut().ip += 2; // opcode + 1 byte for u8
        Ok(operand)
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

    /// Execute a thunk (field closure) synchronously and return its result.
    fn execute_thunk_sync(
        &mut self,
        closure_index: ClosureIndex,
        self_obj: Option<ObjectIndex>,
        super_obj: Option<ObjectIndex>,
    ) -> Result<Value, RuntimeError> {
        self.push(Value::Closure(closure_index))?;
        self.push(self_obj.map(Value::Object).unwrap_or(Value::Null))?;
        self.push(super_obj.map(Value::Object).unwrap_or(Value::Null))?;
        let target_frame_count = self.frame_count;
        self.call_closure(closure_index, 2, self_obj, super_obj)?;
        self.interpret_until(target_frame_count)
    }

    /// Call a Value (Closure or NativeFunction) with a single argument and return its result.
    fn call_value_with_one_arg(&mut self, func: Value, arg: Value) -> Result<Value, RuntimeError> {
        match func {
            Value::Closure(closure_index) => {
                self.push(func)?;
                self.push(arg)?;
                let target_frame_count = self.frame_count;
                self.call_closure(closure_index, 1, None, None)?;
                self.interpret_until(target_frame_count)
            }
            Value::NativeFunction(id) => {
                let span = self.get_current_span();
                let source_id = self.current_chunk().source_id.to_string();
                call_native(id, &[arg], &mut self.memory_manager, span, source_id)
            }
            _ => Err(RuntimeError {
                span: self.get_current_span(),
                message: format!("keyF argument must be a function, got {:?}", func),
                source_id: self.current_chunk().source_id.to_string(),
            }),
        }
    }

    /// Call a closure with the given number of arguments and optional object context.
    /// Stack layout: [..., closure, arg0, arg1, ..., argN]
    /// The closure stays on the stack and becomes slot 0 of the new frame.
    fn call_closure(
        &mut self,
        closure_index: ClosureIndex,
        arg_count: usize,
        self_obj: Option<ObjectIndex>,
        super_obj: Option<ObjectIndex>,
    ) -> Result<(), RuntimeError> {
        let closure = self.memory_manager.load_closure(closure_index);
        let function = self.memory_manager.load_function(closure.function);

        // Validate arity
        if arg_count != function.arity as usize {
            return Err(RuntimeError {
                span: self.get_current_span(),
                message: format!(
                    "Function expects {} arguments, got {}",
                    function.arity, arg_count
                ),
                source_id: self.current_chunk().source_id.to_string(),
            });
        }

        // Check stack depth
        if self.frame_count >= MAX_FRAMES {
            return Err(RuntimeError {
                span: self.get_current_span(),
                message: format!(
                    "Stack overflow - exceeded maximum call depth of {}",
                    MAX_FRAMES
                ),
                source_id: self.current_chunk().source_id.to_string(),
            });
        }

        // Calculate stack_base: points to the closure on the stack
        // Stack: [..., closure, arg0, arg1, ..., argN-1]
        //              ^stack_base                      ^stack top
        let stack_base = self.stack.len() - arg_count - 1;

        // Create new call frame
        let new_frame = CallFrame::new(closure_index, 0, stack_base, self_obj, super_obj);

        // Push frame
        if self.frame_count < self.frames.len() {
            self.frames[self.frame_count] = new_frame;
        } else {
            self.frames.push(new_frame);
        }
        self.frame_count += 1;

        Ok(())
    }

    /// Return from the current function with the given return value.
    /// Returns true if this was the top-level script (frame_count == 0 after return).
    fn return_from_function(&mut self, return_value: Value) -> bool {
        // Get current frame before popping
        let frame = self.current_frame();
        let stack_base = frame.stack_base;

        // Pop the frame
        self.frame_count -= 1;

        // Close any open upvalues for this frame
        self.close_upvalues(stack_base);

        // Clean up the stack (remove closure and args)
        self.stack.truncate(stack_base);

        // Push return value
        self.stack.push(return_value);

        // Return true if we've returned from the top-level script
        self.frame_count == 0
    }

    /// Close upvalues for stack slots at or above last_slot.
    /// This captures values from the stack into the upvalue's closed_value,
    /// effectively moving them from stack to heap.
    fn close_upvalues(&mut self, last_slot: usize) {
        while let Some(upvalue_index) = self.open_upvalues {
            let upvalue = self.memory_manager.load_upvalue(upvalue_index);

            if let Some(location) = upvalue.stack_location {
                if location >= last_slot {
                    // This upvalue needs to be closed
                    let value = self.stack[location];
                    let next = upvalue.next;

                    // Close the upvalue (move from stack to heap)
                    self.memory_manager
                        .load_upvalue_mut(upvalue_index)
                        .close(value);

                    // Remove from open list
                    self.open_upvalues = next;
                } else {
                    // Lower stack slots remain open
                    break;
                }
            } else {
                // Already closed, move to next
                self.open_upvalues = upvalue.next;
            }
        }
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

    /// Swap two stack slots and update any open upvalues pointing to them
    fn swap_upvalues(&mut self, slot_a: usize, slot_b: usize) {
        let mut upvalue = self.open_upvalues;
        while let Some(upvalue_index) = upvalue {
            let managed_upvalue = self.memory_manager.load_upvalue_mut(upvalue_index);
            if managed_upvalue.stack_location == Some(slot_a) {
                managed_upvalue.stack_location = Some(slot_b);
            } else if managed_upvalue.stack_location == Some(slot_b) {
                managed_upvalue.stack_location = Some(slot_a);
            }
            upvalue = managed_upvalue.next;
        }
    }

    pub fn force_value(&mut self, val: Value) -> Result<Value, RuntimeError> {
        if let Value::Import(import_idx) = val {
            // 1. Initial checks and set evaluating flag
            let (target_path_str, already_cached) = {
                let import = self.memory_manager.load_import(import_idx);

                // Check if already cached
                if let Some(cached) = import.cached_result {
                    return Ok(cached);
                }

                // Detect cyclic imports
                if import.evaluating.get() {
                    let path_str_idx = import.path;
                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();
                    return Err(RuntimeError {
                        span: self.get_current_span(),
                        message: format!(
                            "Cyclic import detected: file '{}' is already being evaluated",
                            path_str
                        ),
                        source_id: self.current_chunk().source_id.to_string(),
                    });
                }

                // Mark as evaluating
                import.evaluating.set(true);

                let path_str_idx = import.path;
                (
                    self.memory_manager.load_string(path_str_idx).to_string(),
                    false,
                )
            };

            if already_cached {
                return Ok(self
                    .memory_manager
                    .load_import(import_idx)
                    .cached_result
                    .unwrap());
            }

            // 2. Read the file
            let content = match std::fs::read_to_string(&target_path_str) {
                Ok(content) => content,
                Err(e) => {
                    self.memory_manager
                        .load_import(import_idx)
                        .evaluating
                        .set(false);
                    return Err(RuntimeError {
                        span: self.get_current_span(),
                        message: format!(
                            "Failed to read imported file '{}': {}",
                            target_path_str, e
                        ),
                        source_id: self.current_chunk().source_id.to_string(),
                    });
                }
            };

            // Protect the current VM roots from GC during nested compilation and execution
            let mut roots = Vec::from(self.stack.clone());
            roots.push(val); // Protect the value we are currently forcing
            for i in 0..self.frame_count {
                roots.push(Value::Closure(self.frames[i].closure));
            }

            let mut open_upvalue_roots = Vec::new();
            let mut upvalue = self.open_upvalues;
            while let Some(upvalue_index) = upvalue {
                open_upvalue_roots.push(upvalue_index);
                upvalue = self.memory_manager.load_upvalue(upvalue_index).next;
            }

            self.memory_manager
                .push_external_roots(roots, open_upvalue_roots);

            // 3. Compile the file
            let mut scanner = scanner::Scanner::new(&content, &target_path_str);
            let compiler = compiler::Compiler::new(&mut scanner, &target_path_str);
            let chunk = match compiler.compile(&mut self.memory_manager) {
                Ok(chunk) => chunk,
                Err(e) => {
                    self.memory_manager.pop_external_roots();
                    self.memory_manager
                        .load_import(import_idx)
                        .evaluating
                        .set(false);
                    return Err(RuntimeError {
                        span: self.get_current_span(),
                        message: format!(
                            "Failed to compile imported file '{}': {:?}",
                            target_path_str, e
                        ),
                        source_id: self.current_chunk().source_id.to_string(),
                    });
                }
            };

            // 4. Execute the chunk
            let dummy_memory_manager = memory_manager::MemoryManager::new();
            let actual_memory_manager =
                std::mem::replace(&mut self.memory_manager, dummy_memory_manager);

            let mut sub_vm = VirtualMachine::new(chunk, actual_memory_manager);
            let result = sub_vm.interpret();

            self.memory_manager = sub_vm.memory_manager;
            self.memory_manager.pop_external_roots();

            match result {
                Ok(evaluated_value) => {
                    let import = self.memory_manager.load_import_mut(import_idx);
                    import.cached_result = Some(evaluated_value);
                    import.evaluating.set(false);
                    Ok(evaluated_value)
                }
                Err(e) => {
                    self.memory_manager
                        .load_import_mut(import_idx)
                        .evaluating
                        .set(false);
                    Err(e)
                }
            }
        } else {
            Ok(val)
        }
    }

    /// Main interpretation loop
    pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
        self.interpret_until(0)
    }

    fn interpret_until(&mut self, target_frame_count: usize) -> Result<Value, RuntimeError> {
        loop {
            // Store the start IP of this instruction for error reporting
            self.instruction_start_ip = self.current_frame().ip;

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
                Opcode::LoadSelf => {
                    let self_obj = self.current_frame().self_obj.ok_or_else(|| RuntimeError {
                        span: self.get_current_span(),
                        message: "'self' used outside of object scope".to_string(),
                        source_id: self.current_chunk().source_id.to_string(),
                    })?;
                    self.push(Value::Object(self_obj))?;
                    self.advance_pc();
                }

                Opcode::LoadSuper => {
                    let super_obj = self.current_frame().super_obj.ok_or_else(|| RuntimeError {
                        span: self.get_current_span(),
                        message: "'super' used outside of object scope".to_string(),
                        source_id: self.current_chunk().source_id.to_string(),
                    })?;
                    self.push(Value::Object(super_obj))?;
                    self.advance_pc();
                }

                Opcode::SuperIndex => {
                    let field_name = self.pop_forced()?; // Pop property name

                    let (self_obj_key, super_obj_key) = {
                        let frame = self.current_frame();
                        let s = frame.self_obj.ok_or_else(|| RuntimeError {
                            span: self.get_current_span(),
                            message: "'super' used outside of object scope".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        })?;
                        let su = frame.super_obj.ok_or_else(|| RuntimeError {
                            span: self.get_current_span(),
                            message: "'super' used outside of object scope".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        })?;
                        (s, su)
                    };

                    if let Value::String(field_key) = field_name {
                        let field = self
                            .memory_manager
                            .load_object(super_obj_key)
                            .get_field(&field_key)
                            .cloned();

                        if let Some(field) = field {
                            match field.value {
                                Value::Closure(closure_idx) => {
                                    self.advance_pc();
                                    self.push(Value::Closure(closure_idx))?;
                                    self.push(Value::Object(self_obj_key))?; // ORIGINAL self
                                    let super_val =
                                        field.super_obj.map(Value::Object).unwrap_or(Value::Null);
                                    self.push(super_val)?;
                                    self.call_closure(
                                        closure_idx,
                                        2,
                                        Some(self_obj_key),
                                        field.super_obj,
                                    )?;
                                    continue;
                                }
                                _ => {
                                    self.push(field.value.clone())?;
                                }
                            }
                        } else {
                            self.push(Value::Null)?;
                        }
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Super index must be a string, got {:?}", field_name),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }
                    self.advance_pc();
                }

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
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;

                    // Check for different addition types
                    match (&a, &b) {
                        // Object merging (according to Jsonnet spec)
                        (Value::Object(left_key), Value::Object(right_key)) => {
                            let left_key = *left_key;
                            let right_key = *right_key;
                            let (left_object, right_object) = (
                                self.memory_manager.load_object(left_key),
                                self.memory_manager.load_object(right_key),
                            );
                            // Create merged properties starting with left object
                            let mut merged_properties = left_object.properties.clone();

                            // Override/add properties from right object
                            for (key, right_field) in &right_object.properties {
                                // Visibility inheritance: if right visibility is ':', inherit from left if it exists
                                let visibility =
                                    if right_field.visibility == FieldVisibility::Visible {
                                        if let Some(left_field) = left_object.get_field(key) {
                                            left_field.visibility
                                        } else {
                                            FieldVisibility::Visible
                                        }
                                    } else {
                                        right_field.visibility
                                    };

                                merged_properties.insert(
                                    *key,
                                    ObjectField {
                                        value: right_field.value.clone(),
                                        super_obj: Some(left_key), // Field's super is the left object
                                        visibility,
                                    },
                                );
                            }

                            // Concatenate assertions: left then right
                            let mut merged_assertions = left_object.assertions.clone();
                            merged_assertions.extend(right_object.assertions.clone());

                            let merged_allocation = self
                                .memory_manager
                                .allocate_object_full(merged_properties, merged_assertions);
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
                                | Value::Closure(_)
                                | Value::NativeFunction(_)
                                | Value::Import(_)
                                | Value::Binary(_) => unreachable!(),
                            };
                            let b_str = match &b {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_)
                                | Value::Array(_)
                                | Value::Function(_)
                                | Value::Closure(_)
                                | Value::NativeFunction(_)
                                | Value::Import(_)
                                | Value::Binary(_) => unreachable!(),
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
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? - self.to_number(b)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Mul => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? * self.to_number(b)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Div => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
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

                Opcode::Mod => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    // If LHS is a string, treat % as string formatting (Python-style)
                    if matches!(a, Value::String(_)) {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let result = native::std_format_public(
                            a,
                            b,
                            &mut self.memory_manager,
                            span,
                            source_id,
                        )?;
                        self.push(result)?;
                        self.advance_pc();
                    } else {
                        let b_num = self.to_number(b)?;
                        if b_num == 0.0 {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Modulo by zero".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                        let result = self.to_number(a)? % b_num;
                        self.push(Value::Number(result))?;
                        self.advance_pc();
                    }
                }

                // Comparison operations
                Opcode::Lt => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? < self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Le => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? <= self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Gt => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? > self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ge => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)? >= self.to_number(b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                // Equality operations
                Opcode::Eq => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.values_equal(&a, &b);
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ne => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = !self.values_equal(&a, &b);
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                // String operations
                Opcode::StringConcat => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;

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
                                Value::Function(_) | Value::NativeFunction(_) => {
                                    "{function}".to_string()
                                }
                                Value::Closure(_) => "{closure}".to_string(),
                                Value::Import(_) => "{import}".to_string(),
                                Value::Binary(_) => "{binary}".to_string(),
                            };
                            let b_str = match &b {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                                Value::Array(_) => "{array}".to_string(),
                                Value::Function(_) | Value::NativeFunction(_) => {
                                    "{function}".to_string()
                                }
                                Value::Closure(_) => "{closure}".to_string(),
                                Value::Import(_) => "{import}".to_string(),
                                Value::Binary(_) => "{binary}".to_string(),
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
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
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
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let shift_count = (self.to_integer(b)? % 64) as u32;
                    let result = (self.to_integer(a)? >> shift_count) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitAnd => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = (self.to_integer(a)? & self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitXor => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = (self.to_integer(a)? ^ self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::BitOr => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = (self.to_integer(a)? | self.to_integer(b)?) as f64;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                // Unary operations
                Opcode::Neg => {
                    let a = self.pop_forced()?;
                    let result = -self.to_number(a)?;
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Pos => {
                    let a = self.pop_forced()?;
                    let result = self.to_number(a)?; // Unary + is essentially a no-op for numbers
                    self.push(Value::Number(result))?;
                    self.advance_pc();
                }

                Opcode::Not => {
                    let a = self.pop_forced()?;
                    let result = !self.is_truthy(a)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::BitNot => {
                    let a = self.pop_forced()?;
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
                    let b_idx = self.stack.len() - 1;
                    let a_idx = self.stack.len() - 2;
                    let b = self.pop()?;
                    let a = self.pop()?;
                    self.push(b)?;
                    self.push(a)?;
                    self.swap_upvalues(a_idx, b_idx);
                    self.advance_pc();
                }

                Opcode::StoreVar => {
                    let slot = self.read_u16_operand()? as usize;
                    let value = self.pop()?;

                    // Calculate absolute stack position for current frame
                    let frame_base = self.current_frame().stack_base;
                    let abs_slot = frame_base + slot;

                    if abs_slot >= self.stack.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "StoreVar: slot {} is out of range (stack size: {})",
                                slot,
                                self.stack.len()
                            ),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }

                    self.stack[abs_slot] = value;
                    // Note: read_u16_operand() already advanced PC by 3 (opcode + u16)
                    // No need to call advance_pc() here
                }

                Opcode::CreateObject => {
                    let field_count = self.read_u16_operand()?;

                    // Pop field_count pairs of (key, value) from the stack
                    let mut properties = std::collections::HashMap::new();

                    for _ in 0..field_count {
                        let value = self.pop()?;
                        let key = self.pop()?;

                        // Ensure key is a string or null
                        match key {
                            Value::String(key_str) => {
                                properties.insert(
                                    key_str,
                                    ObjectField {
                                        value,
                                        super_obj: None,
                                        visibility: FieldVisibility::Visible,
                                    },
                                );
                            }
                            Value::Null => {
                                // Null keys are omitted as per Jsonnet spec
                            }
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "Object key must be a string or null, got {:?}",
                                        key
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
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

                Opcode::Assert => {
                    let closure_val = self.pop()?;
                    let object_val = self.pop()?;

                    match (closure_val, object_val) {
                        (Value::Closure(closure_idx), Value::Object(obj_idx)) => {
                            // Attach the assertion to the object
                            if let Some(obj) = self.memory_manager.get_object_mut(obj_idx) {
                                obj.assertions.push(closure_idx);
                            }

                            // Push object back onto stack (it's the result of this opcode)
                            self.push(Value::Object(obj_idx))?;

                            // Execute assertion immediately for early failure detection.
                            // Object assertions in Jsonnet are conceptually checked during manifestation,
                            // but in an eager VM, construction is effectively the beginning of manifestation.
                            // We use execute_thunk_sync which correctly passes self and super.
                            self.execute_thunk_sync(closure_idx, Some(obj_idx), None)?;
                        }
                        (c, o) => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Invalid operands for Opcode::Assert: expected closure and object, got {:?} and {:?}",
                                    c, o
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }
                    self.advance_pc();
                }

                Opcode::ObjectInsert => {
                    let visibility_byte = self.read_u8_operand()?;
                    let visibility = match visibility_byte {
                        0 => FieldVisibility::Visible,
                        1 => FieldVisibility::Hidden,
                        2 => FieldVisibility::ForceVisible,
                        _ => FieldVisibility::Visible,
                    };

                    let value = self.pop()?;
                    let key = self.pop()?;
                    let object_val = self.pop()?;

                    match object_val {
                        Value::Object(obj_key) => {
                            let obj = self.memory_manager.load_object(obj_key);
                            let mut properties = obj.properties.clone();
                            let assertions = obj.assertions.clone();

                            match key {
                                Value::String(key_str) => {
                                    properties.insert(
                                        key_str,
                                        ObjectField {
                                            value,
                                            super_obj: None,
                                            visibility,
                                        },
                                    );
                                }
                                Value::Null => {
                                    // Null keys are omitted
                                }
                                _ => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Object key must be a string or null, got {:?}",
                                            key
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            }

                            let object_allocation = self
                                .memory_manager
                                .allocate_object_full(properties, assertions);
                            self.push(Value::Object(object_allocation.index))?;

                            if object_allocation.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (ObjectInsert)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Expected object for ObjectInsert, got {:?}",
                                    object_val
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }
                    // No advance_pc() here because read_u8_operand already moved it to the start of the next instruction
                    continue;
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
                    let field_name = self.pop_forced()?; // Property name to access
                    let object_value = self.pop_forced()?; // Object to index into

                    // Ensure we have an object
                    if let Value::Object(object_key) = object_value {
                        // Ensure field name is a string
                        if let Value::String(field_key) = field_name {
                            let field = self
                                .memory_manager
                                .load_object(object_key)
                                .get_field(&field_key)
                                .cloned();

                            if let Some(field) = field {
                                match field.value {
                                    Value::Closure(closure_idx) => {
                                        // It's a thunk! We need to call it with (self, super)
                                        self.advance_pc(); // Advance past ObjectIndex before calling
                                        self.push(Value::Closure(closure_idx))?;
                                        self.push(Value::Object(object_key))?; // self
                                        let super_val = field
                                            .super_obj
                                            .map(Value::Object)
                                            .unwrap_or(Value::Null);
                                        self.push(super_val)?; // super
                                        self.call_closure(
                                            closure_idx,
                                            2,
                                            Some(object_key),
                                            field.super_obj,
                                        )?;
                                        continue;
                                    }
                                    _ => {
                                        // It's a raw value
                                        self.push(field.value.clone())?;
                                    }
                                }
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
                    let index_value = self.pop_forced()?;
                    let container_value = self.pop_forced()?;

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
                        Value::Binary(binary_key) => {
                            // Binary indexing with number
                            if let Value::Number(index_num) = index_value {
                                // Check for negative index
                                if index_num < 0.0 {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Binary index cannot be negative, got {}",
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
                                            "Binary index must be an integer, got {}",
                                            index_num
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }

                                let index = index_num as usize;
                                let binary = self.memory_manager.load_binary(binary_key);

                                // Bounds check
                                if index >= binary.data.len() {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "Binary index {} out of bounds (length: {})",
                                            index,
                                            binary.data.len()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }

                                self.push(Value::Number(binary.data[index] as f64))?;
                            } else {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "Binary index must be a number, got {:?}",
                                        index_value
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        }
                        Value::Object(object_key) => {
                            // Object indexing with string
                            if let Value::String(field_key) = index_value {
                                let field = self
                                    .memory_manager
                                    .load_object(object_key)
                                    .get_field(&field_key)
                                    .cloned();

                                if let Some(field) = field {
                                    match field.value {
                                        Value::Closure(closure_idx) => {
                                            self.advance_pc();
                                            self.push(Value::Closure(closure_idx))?;
                                            self.push(Value::Object(object_key))?; // self
                                            let super_val = field
                                                .super_obj
                                                .map(Value::Object)
                                                .unwrap_or(Value::Null);
                                            self.push(super_val)?;
                                            self.call_closure(
                                                closure_idx,
                                                2,
                                                Some(object_key),
                                                field.super_obj,
                                            )?;
                                            continue;
                                        }
                                        _ => {
                                            self.push(field.value.clone())?;
                                        }
                                    }
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

                Opcode::ArrayLength => {
                    let array_value = self.pop_forced()?;

                    match array_value {
                        Value::Array(array_key) => {
                            let array = self.memory_manager.load_array(array_key);
                            let length = array.len() as f64;
                            self.push(Value::Number(length))?;
                        }
                        Value::Binary(binary_key) => {
                            let binary = self.memory_manager.load_binary(binary_key);
                            let length = binary.data.len() as f64;
                            self.push(Value::Number(length))?;
                        }
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Cannot get length of non-array value: {:?}",
                                    array_value
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }

                    self.advance_pc();
                }

                Opcode::ArrayAppend => {
                    let value_to_append = self.pop()?;
                    let array_value = self.pop()?;

                    match array_value {
                        Value::Array(array_key) => {
                            let array = self.memory_manager.load_array(array_key);
                            // Create new array with appended value
                            let mut new_elements = array.elements.clone();
                            new_elements.push(value_to_append);

                            let new_array_allocation =
                                self.memory_manager.allocate_array(new_elements);
                            self.push(Value::Array(new_array_allocation.index))?;

                            if new_array_allocation.should_garbage_collect {
                                #[cfg(feature = "gc_debug")]
                                {
                                    eprintln!(
                                        "[VirtualMachine] Running GC at PC={} (ArrayAppend)",
                                        self.current_frame().ip
                                    );
                                }
                                self.run_garbage_collection();
                            }
                        }
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Cannot append to non-array value: {:?}",
                                    array_value
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }

                    self.advance_pc();
                }

                Opcode::ObjectMerge => {
                    let right_value = self.pop_forced()?; // Right-hand side object
                    let left_value = self.pop_forced()?; // Left-hand side object

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
                        for (key, right_field) in &right_object.properties {
                            // Visibility inheritance: if right visibility is ':', inherit from left if it exists
                            let visibility = if right_field.visibility == FieldVisibility::Visible {
                                if let Some(left_field) = left_object.get_field(key) {
                                    left_field.visibility
                                } else {
                                    FieldVisibility::Visible
                                }
                            } else {
                                right_field.visibility
                            };

                            merged_properties.insert(
                                *key,
                                ObjectField {
                                    value: right_field.value,
                                    super_obj: Some(left_key), // Field's super is the left object
                                    visibility,
                                },
                            );
                        }

                        // Concatenate assertions: left then right
                        let mut merged_assertions = left_object.assertions.clone();
                        merged_assertions.extend(right_object.assertions.clone());

                        let merged_allocation = self
                            .memory_manager
                            .allocate_object_full(merged_properties, merged_assertions);
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

                Opcode::StdCall => {
                    let frame = self.current_frame();
                    let chunk = self.current_chunk();

                    if frame.ip + 3 >= chunk.count() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Invalid bytecode - missing StdCall operands".to_string(),
                            source_id: chunk.source_id.to_string(),
                        });
                    }

                    let func_id_val = chunk.read_u16(frame.ip + 1).unwrap();
                    let arg_count = chunk.read_u8(frame.ip + 3).unwrap() as usize;
                    self.current_frame_mut().ip += 4; // opcode + u16 + u8

                    let func_id =
                        chunk::NativeFuncId::from_u16(func_id_val).ok_or_else(|| RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Invalid native function ID: {}", func_id_val),
                            source_id: self.current_chunk().source_id.to_string(),
                        })?;

                    // Extract arguments from stack
                    let args = self.stack[self.stack.len() - arg_count..].to_vec();
                    // Pop arguments and the native function value itself
                    for _ in 0..=arg_count {
                        self.pop()?;
                    }

                    // Handle std.get: field value may be a thunk closure
                    if func_id == chunk::NativeFuncId::Get {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let o_val = args[0];
                        let f_val = args[1];
                        let default_val = args[2];
                        let inc_hidden_val = args[3];

                        let o_idx = match o_val {
                            Value::Object(o) => o,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "std.get() first argument must be an object"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let field_name = match f_val {
                            Value::String(s_idx) => {
                                self.memory_manager.load_string(s_idx).to_string()
                            }
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "std.get() second argument must be a string"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let inc_hidden = match inc_hidden_val {
                            Value::Boolean(b) => b,
                            Value::Null => true,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "std.get() fourth argument must be a boolean or null"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };

                        let obj = self.memory_manager.load_object(o_idx);
                        let found: Option<(Value, FieldVisibility, Option<ObjectIndex>)> = obj
                            .properties
                            .iter()
                            .find(|(k, _)| {
                                self.memory_manager.load_string(**k) == field_name.as_str()
                            })
                            .map(|(_, f)| (f.value, f.visibility, f.super_obj));

                        let result = match found {
                            Some((val, visibility, super_obj)) => {
                                if inc_hidden || visibility != FieldVisibility::Hidden {
                                    // Force-evaluate thunk if needed
                                    match val {
                                        Value::Closure(closure_idx) => {
                                            let result = self.execute_thunk_sync(
                                                closure_idx,
                                                Some(o_idx),
                                                super_obj,
                                            )?;
                                            self.push(result)?;
                                            continue;
                                        }
                                        _ => val,
                                    }
                                } else {
                                    default_val
                                }
                            }
                            None => default_val,
                        };
                        self.push(result)?;
                        continue;
                    }

                    // Handle std.format: when vals is an object, field values may be thunks.
                    // Pre-evaluate all object field values before passing to native.
                    if func_id == chunk::NativeFuncId::Format && args.len() >= 2 {
                        let vals_val = args[1];
                        if let Value::Object(o_idx) = vals_val {
                            let span = self.get_current_span();
                            let source_id = self.current_chunk().source_id.to_string();
                            // Collect all (key_string_idx, raw_value, super_obj) pairs
                            let pairs: Vec<(StringIndex, Value, Option<ObjectIndex>)> = self
                                .memory_manager
                                .load_object(o_idx)
                                .properties
                                .iter()
                                .map(|(k, f)| (*k, f.value, f.super_obj))
                                .collect();
                            // Evaluate each field
                            let mut evaluated: Vec<(StringIndex, Value)> =
                                Vec::with_capacity(pairs.len());
                            for (k, v, super_obj) in pairs {
                                let ev = match v {
                                    Value::Closure(closure_idx) => self.execute_thunk_sync(
                                        closure_idx,
                                        Some(o_idx),
                                        super_obj,
                                    )?,
                                    other => other,
                                };
                                evaluated.push((k, ev));
                            }
                            // Build a new object with evaluated values and pass to format
                            let mut properties = std::collections::HashMap::new();
                            for (k, v) in evaluated {
                                properties.insert(
                                    k,
                                    memory_manager::ObjectField {
                                        value: v,
                                        super_obj: None,
                                        visibility: FieldVisibility::Visible,
                                    },
                                );
                            }
                            let new_obj_alloc = self
                                .memory_manager
                                .allocate_object_with_properties(properties);
                            let new_obj_val = Value::Object(new_obj_alloc.index);
                            // Now call format with the new object
                            let new_args = vec![args[0], new_obj_val];
                            let result = call_native(
                                func_id,
                                &new_args,
                                &mut self.memory_manager,
                                span,
                                source_id,
                            )?;
                            self.push(result)?;
                            continue;
                        }
                    }

                    if func_id == chunk::NativeFuncId::MakeArray {
                        // Handle MakeArray natively within the VM to allow calling closures
                        let sz_val = args[0];
                        let func_val = args[1];

                        let sz = match sz_val {
                            Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.makeArray expected positive integer for size, got {}",
                                        sz_val
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };

                        // Create ephemeral chunk to loop and create the array
                        let mut make_array_chunk = chunk::Chunk::new("<makearray>");
                        let func_idx = make_array_chunk.add_constant(func_val);

                        make_array_chunk.write_opcode_u16(Opcode::CreateArray, 0, 0..0);

                        for i in 0..sz {
                            let i_idx = make_array_chunk.add_constant(Value::Number(i as f64));
                            make_array_chunk.write_opcode_u16(
                                Opcode::LoadConst,
                                func_idx as u16,
                                0..0,
                            );
                            make_array_chunk.write_opcode_u16(
                                Opcode::LoadConst,
                                i_idx as u16,
                                0..0,
                            );
                            make_array_chunk.write_opcode_u8_u8(Opcode::Call, 1, 0, 0..0);
                            make_array_chunk.write_opcode(Opcode::ArrayAppend, 0..0);
                        }

                        make_array_chunk.write_opcode(Opcode::Return, 0..0);

                        let owned_chunk = make_array_chunk.into_owned();
                        let function_allocation =
                            self.memory_manager
                                .allocate_function(None, 0, 0, owned_chunk);
                        let function_index = function_allocation.index;

                        // Temporarily root the function while we allocate the closure
                        self.memory_manager
                            .external_roots
                            .push(vec![Value::Function(function_index)]);

                        // Create a closure from the function to invoke it
                        let closure_allocation = self
                            .memory_manager
                            .allocate_closure(function_index, Vec::new());
                        let closure_index = closure_allocation.index;

                        // Remove the temporary root
                        self.memory_manager.external_roots.pop();

                        // Call it by pushing a frame!
                        let new_frame =
                            CallFrame::new(closure_index, 0, self.stack.len(), None, None);

                        if self.frame_count < self.frames.len() {
                            self.frames[self.frame_count] = new_frame;
                        } else {
                            self.frames.push(new_frame);
                        }
                        self.frame_count += 1;

                        if function_allocation.should_garbage_collect
                            || closure_allocation.should_garbage_collect
                        {
                            self.run_garbage_collection();
                        }

                        continue;
                    }

                    // Handle std.sort with keyF
                    if func_id == chunk::NativeFuncId::Sort && args.len() == 2 {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.sort() expected array as first argument"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();

                        // Compute keys for each element by calling keyF
                        let mut keys: Vec<Value> = Vec::with_capacity(elements.len());
                        for &elem in &elements {
                            // Root accumulated data before each call to protect from GC
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&keys);
                            roots.push(key_f);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let key = self.call_value_with_one_arg(key_f, elem);
                            self.memory_manager.pop_external_roots();
                            keys.push(key?);
                        }

                        // Sort elements by their computed keys
                        let mut indexed: Vec<usize> = (0..elements.len()).collect();
                        let mm = &self.memory_manager;
                        indexed.sort_by(|&a, &b| native::compare_values(keys[a], keys[b], mm));
                        let sorted: Vec<Value> = indexed.iter().map(|&i| elements[i]).collect();
                        let arr_alloc = self.memory_manager.allocate_array(sorted);
                        self.push(Value::Array(arr_alloc.index))?;
                        continue;
                    }

                    // Handle std.uniq with keyF
                    if func_id == chunk::NativeFuncId::Uniq && args.len() == 2 {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.uniq() expected array as first argument"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();

                        let mut result: Vec<Value> = Vec::new();
                        let mut last_key: Option<Value> = None;
                        for &elem in &elements {
                            // Root accumulated data before each call to protect from GC
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&result);
                            if let Some(lk) = last_key {
                                roots.push(lk);
                            }
                            roots.push(key_f);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let key = self.call_value_with_one_arg(key_f, elem);
                            self.memory_manager.pop_external_roots();
                            let key = key?;
                            if let Some(lk) = last_key {
                                if native::values_equal(lk, key, &self.memory_manager) {
                                    continue;
                                }
                            }
                            last_key = Some(key);
                            result.push(elem);
                        }
                        let arr_alloc = self.memory_manager.allocate_array(result);
                        self.push(Value::Array(arr_alloc.index))?;
                        continue;
                    }

                    // Handle std.objectValues / std.objectValuesAll / std.objectKeysValues:
                    // field values may be thunk closures that need to be force-evaluated.
                    if (func_id == chunk::NativeFuncId::ObjectValues
                        || func_id == chunk::NativeFuncId::ObjectValuesAll
                        || func_id == chunk::NativeFuncId::ObjectKeysValues)
                        && !args.is_empty()
                    {
                        if let Value::Object(o_idx) = args[0] {
                            let span = self.get_current_span();
                            let source_id = self.current_chunk().source_id.to_string();
                            // Collect all (key, raw_value, super_obj, visibility) tuples
                            let field_data: Vec<(
                                chunk::StringIndex,
                                Value,
                                Option<ObjectIndex>,
                                FieldVisibility,
                            )> = self
                                .memory_manager
                                .load_object(o_idx)
                                .properties
                                .iter()
                                .map(|(k, f)| (*k, f.value, f.super_obj, f.visibility))
                                .collect();
                            // Evaluate each field's thunk
                            let mut evaluated: Vec<(chunk::StringIndex, Value, FieldVisibility)> =
                                Vec::with_capacity(field_data.len());
                            for (k, v, super_obj, vis) in field_data {
                                let ev = match v {
                                    Value::Closure(closure_idx) => self.execute_thunk_sync(
                                        closure_idx,
                                        Some(o_idx),
                                        super_obj,
                                    )?,
                                    other => other,
                                };
                                evaluated.push((k, ev, vis));
                            }
                            // Rebuild the object with evaluated values
                            let mut properties = std::collections::HashMap::new();
                            for (k, v, vis) in evaluated {
                                properties.insert(
                                    k,
                                    memory_manager::ObjectField {
                                        value: v,
                                        super_obj: None,
                                        visibility: vis,
                                    },
                                );
                            }
                            let new_obj_alloc = self
                                .memory_manager
                                .allocate_object_with_properties(properties);
                            let new_obj_val = Value::Object(new_obj_alloc.index);
                            let new_args = vec![new_obj_val];
                            let result = call_native(
                                func_id,
                                &new_args,
                                &mut self.memory_manager,
                                span,
                                source_id,
                            )?;
                            self.push(result)?;
                            continue;
                        }
                    }

                    // Call native function
                    let span = self.get_current_span();
                    let source_id = self.current_chunk().source_id.to_string();
                    let result =
                        call_native(func_id, &args, &mut self.memory_manager, span, source_id)?;
                    self.push(result)?;
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

                Opcode::Import => {
                    // Read constant index, which points to a string constant (the path)
                    let const_idx = self.read_u16_operand()?;

                    let path_str_idx = match self.current_chunk().constants.get(const_idx as usize)
                    {
                        Some(Value::String(idx)) => *idx,
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "Import operand must be a string constant".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    // Resolve the path relative to the current chunk's source_id
                    let current_dir = std::path::Path::new(&self.current_chunk().source_id)
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let target_path = current_dir.join(&path_str);
                    let target_path_str = target_path.to_string_lossy().to_string();

                    let import_idx = self.memory_manager.allocate_import(&target_path_str).index;
                    self.push(Value::Import(import_idx))?;
                }

                Opcode::ImportStr => {
                    // Read constant index, which points to a string constant (the path)
                    let const_idx = self.read_u16_operand()?;

                    let path_str_idx = match self.current_chunk().constants.get(const_idx as usize)
                    {
                        Some(Value::String(idx)) => *idx,
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "ImportStr operand must be a string constant".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    // Resolve the path relative to the current chunk's source_id
                    let current_dir = std::path::Path::new(&self.current_chunk().source_id)
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let target_path = current_dir.join(&path_str);
                    let target_path_str = target_path.to_string_lossy().to_string();

                    // Read the file content
                    let content =
                        std::fs::read_to_string(&target_path_str).map_err(|e| RuntimeError {
                            span: self.get_current_span(),
                            message: format!("Failed to read file '{}': {}", target_path_str, e),
                            source_id: self.current_chunk().source_id.to_string(),
                        })?;

                    // Allocate content as string
                    let allocation_result = self.memory_manager.allocate_string(&content);
                    self.push(Value::String(allocation_result.index))?;

                    if allocation_result.should_garbage_collect {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[VirtualMachine] Running GC at PC={} (ImportStr allocation)",
                                self.current_frame().ip
                            );
                        }
                        self.run_garbage_collection();
                    }
                }

                Opcode::ImportBin => {
                    // Read constant index, which points to a string constant (the path)
                    let const_idx = self.read_u16_operand()?;

                    let path_str_idx = match self.current_chunk().constants.get(const_idx as usize)
                    {
                        Some(Value::String(idx)) => *idx,
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: "ImportBin operand must be a string constant".to_string(),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    // Resolve the path relative to the current chunk's source_id
                    let current_dir = std::path::Path::new(&self.current_chunk().source_id)
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let target_path = current_dir.join(&path_str);
                    let target_path_str = target_path.to_string_lossy().to_string();

                    // Read the file content as binary
                    let content = std::fs::read(&target_path_str).map_err(|e| RuntimeError {
                        span: self.get_current_span(),
                        message: format!("Failed to read file '{}': {}", target_path_str, e),
                        source_id: self.current_chunk().source_id.to_string(),
                    })?;

                    // Allocate content as binary object
                    let allocation_result = self.memory_manager.allocate_binary(content);
                    self.push(Value::Binary(allocation_result.index))?;

                    if allocation_result.should_garbage_collect {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[VirtualMachine] Running GC at PC={} (ImportBin allocation)",
                                self.current_frame().ip
                            );
                        }
                        self.run_garbage_collection();
                    }
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
                    if !self.is_truthy(condition)? {
                        let frame = self.current_frame_mut();
                        frame.ip = (frame.ip as i32 + offset) as usize;
                    }
                    // If truthy, IP already advanced past jump instruction
                }

                Opcode::JumpIfTrue => {
                    let offset = self.read_i32_operand()?;
                    let condition = self.pop()?;
                    if self.is_truthy(condition)? {
                        let frame = self.current_frame_mut();
                        frame.ip = (frame.ip as i32 + offset) as usize;
                    }
                    // If falsy, IP already advanced past jump instruction
                }

                Opcode::Call => {
                    // Read operands: positional_count and named_count
                    let frame = self.current_frame();
                    let chunk = self.current_chunk();

                    if frame.ip + 2 >= chunk.count() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Invalid bytecode - missing Call operands".to_string(),
                            source_id: chunk.source_id.to_string(),
                        });
                    }

                    let positional_count = chunk.code[frame.ip + 1] as usize;
                    let named_count = chunk.code[frame.ip + 2] as usize;
                    self.current_frame_mut().ip += 3; // opcode + 2 bytes

                    // For now, only support positional arguments
                    if named_count > 0 {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Named arguments not yet implemented".to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }

                    let arg_count = positional_count;

                    // Get callee from stack (it's at position: stack.len() - arg_count - 1)
                    let callee_position = self.stack.len() - arg_count - 1;
                    if callee_position >= self.stack.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "Invalid stack access for callee at position {} (stack size: {})",
                                callee_position,
                                self.stack.len()
                            ),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }

                    let mut callee = self.stack[callee_position];
                    callee = self.force_value(callee)?;
                    self.stack[callee_position] = callee; // update stack

                    match callee {
                        Value::Closure(closure_index) => {
                            self.call_closure(closure_index, arg_count, None, None)?;
                        }
                        Value::NativeFunction(id) => {
                            // Extract arguments from stack
                            let args = self.stack[self.stack.len() - arg_count..].to_vec();
                            // Pop arguments and callee
                            for _ in 0..=arg_count {
                                self.pop()?;
                            }
                            // Call native function
                            let span = self.get_current_span();
                            let source_id = self.current_chunk().source_id.to_string();
                            let result =
                                call_native(id, &args, &mut self.memory_manager, span, source_id)?;
                            self.push(result)?;
                        }
                        _ => {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("Cannot call non-function value: {:?}", callee),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                    }
                }

                Opcode::Return => {
                    let return_value = self.pop()?;

                    // Check if this is the top-level script return
                    if self.frame_count == 1 && target_frame_count == 0 {
                        // Top-level return - just return the value
                        return Ok(return_value);
                    }

                    // Return from function
                    let is_script_complete = self.return_from_function(return_value);

                    if self.frame_count == target_frame_count {
                        let val = self.pop()?;
                        return Ok(val);
                    }

                    if is_script_complete {
                        // We've returned from the top-level script
                        let val = self.pop();
                        return val;
                    }
                }

                Opcode::GetUpvalue => {
                    let upvalue_slot = self.read_u16_operand()? as usize;
                    let frame = self.current_frame();
                    let current_closure = self.memory_manager.load_closure(frame.closure);

                    if upvalue_slot >= current_closure.upvalues.len() {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!(
                                "Invalid upvalue slot {} (closure has {} upvalues)",
                                upvalue_slot,
                                current_closure.upvalues.len()
                            ),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    }

                    let upvalue_index = current_closure.upvalues[upvalue_slot];
                    let upvalue = self.memory_manager.load_upvalue(upvalue_index);

                    let value = if let Some(location) = upvalue.stack_location {
                        // Upvalue is open - read from stack
                        if location >= self.stack.len() {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!(
                                    "Invalid upvalue stack location {} (stack size: {})",
                                    location,
                                    self.stack.len()
                                ),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                        let val = self.stack[location];
                        val
                    } else if let Some(closed_value) = upvalue.closed_value {
                        // Upvalue is closed - read from heap
                        let val = closed_value;
                        val
                    } else {
                        return Err(RuntimeError {
                            span: self.get_current_span(),
                            message: "Upvalue has neither stack location nor closed value"
                                .to_string(),
                            source_id: self.current_chunk().source_id.to_string(),
                        });
                    };

                    self.push(value)?;
                }

                Opcode::CloseUpvalue => {
                    // Close upvalue for top of stack (does NOT pop - compiler emits separate Pop)
                    let stack_top = self.stack.len() - 1;
                    self.close_upvalues(stack_top);
                    self.advance_pc();
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
            (Value::Array(a_idx), Value::Array(b_idx)) => {
                if a_idx == b_idx {
                    return true;
                }
                let a_arr = self.memory_manager.load_array(*a_idx);
                let b_arr = self.memory_manager.load_array(*b_idx);
                if a_arr.elements.len() != b_arr.elements.len() {
                    return false;
                }
                for (v_a, v_b) in a_arr.elements.iter().zip(b_arr.elements.iter()) {
                    if !self.values_equal(v_a, v_b) {
                        return false;
                    }
                }
                true
            }
            (Value::Object(a_idx), Value::Object(b_idx)) => {
                if a_idx == b_idx {
                    return true;
                }
                let a_obj = self.memory_manager.load_object(*a_idx);
                let b_obj = self.memory_manager.load_object(*b_idx);
                if a_obj.properties.len() != b_obj.properties.len() {
                    return false;
                }
                for (name, field_a) in a_obj.properties.iter() {
                    match b_obj.properties.get(name) {
                        Some(field_b) => {
                            if !self.values_equal(&field_a.value, &field_b.value) {
                                return false;
                            }
                        }
                        None => return false,
                    }
                }
                true
            }
            (Value::Function(a), Value::Function(b)) => a == b, // compare function indices
            (Value::Closure(a), Value::Closure(b)) => a == b,   // compare closure indices
            (Value::Binary(a), Value::Binary(b)) => a == b, // compare binary object keys (identity)

            // Different types are never equal
            _ => false,
        }
    }

    /// Convert a VM Value to serde_json::Value for JSON output
    fn value_to_json(
        &mut self,
        value: &Value,
        visited: &mut std::collections::HashSet<ObjectIndex>,
    ) -> Result<serde_json::Value, RuntimeError> {
        // Protect current value from GC during its serialization
        self.memory_manager.external_roots.push(vec![value.clone()]);

        let result = (|| {
            let value = self.force_value(value.clone())?;
            match value {
                Value::Null => Ok(serde_json::Value::Null),
                Value::Boolean(b) => Ok(serde_json::Value::Bool(b)),
                Value::Number(n) => serde_json::Number::from_f64(n)
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| RuntimeError {
                        span: 0..0,
                        message: "Invalid number for JSON conversion".to_string(),
                        source_id: "serialization".to_string(),
                    }),
                Value::String(s) => Ok(serde_json::Value::String(
                    self.memory_manager.load_string(s).to_owned(),
                )),
                Value::Object(object_key) => {
                    // Check for circular references
                    if visited.contains(&object_key) {
                        return Err(RuntimeError {
                            span: 0..0,
                            message: "Circular reference detected in object".to_string(),
                            source_id: "serialization".to_string(),
                        });
                    }

                    visited.insert(object_key);
                    let obj = self.memory_manager.load_object(object_key);
                    let properties: Vec<(StringIndex, ObjectField)> = obj
                        .properties
                        .iter()
                        .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                        .map(|(k, field)| (*k, field.clone()))
                        .collect();

                    let mut json_object = serde_json::Map::new();

                    for (key, field) in properties {
                        let field_value = match field.value {
                            Value::Closure(closure_idx) => self.execute_thunk_sync(
                                closure_idx,
                                Some(object_key),
                                field.super_obj,
                            )?,
                            v => v,
                        };
                        let json_value = self.value_to_json(&field_value, visited)?;
                        json_object
                            .insert(self.memory_manager.load_string(key).to_owned(), json_value);
                    }

                    visited.remove(&object_key); // Remove after processing
                    Ok(serde_json::Value::Object(json_object))
                }
                Value::Array(array_key) => {
                    let elements: Vec<Value> =
                        self.memory_manager.load_array(array_key).elements.clone();

                    let mut json_array = Vec::new();

                    for element in &elements {
                        let json_value = self.value_to_json(element, visited)?;
                        json_array.push(json_value);
                    }

                    Ok(serde_json::Value::Array(json_array))
                }
                Value::Binary(binary_key) => {
                    let data = self.memory_manager.load_binary(binary_key).data.clone();
                    let json_array: Vec<serde_json::Value> = data
                        .into_iter()
                        .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                        .collect();
                    Ok(serde_json::Value::Array(json_array))
                }
                _ => Err(RuntimeError {
                    span: self.get_current_span(),
                    message: format!("Cannot serialize value to JSON: {:?}", value),
                    source_id: "serialization".to_string(),
                }),
            }
        })();

        // Remove the protection root
        self.memory_manager.external_roots.pop();
        result
    }

    fn run_garbage_collection(&mut self) {
        let mut roots = Vec::from(self.stack.clone());

        // Add all active frames' closures as roots
        for i in 0..self.frame_count {
            roots.push(Value::Closure(self.frames[i].closure));
        }

        // Collect open upvalues - these are upvalues that point to stack locations
        // and haven't been closed yet. They must be kept alive even if not yet part of a closure.
        let mut open_upvalue_roots = Vec::new();
        let mut upvalue = self.open_upvalues;
        while let Some(upvalue_index) = upvalue {
            open_upvalue_roots.push(upvalue_index);
            upvalue = self.memory_manager.load_upvalue(upvalue_index).next;
        }

        self.memory_manager
            .run_garbage_collect(roots, open_upvalue_roots);
    }

    fn is_truthy(&mut self, value: Value) -> Result<bool, RuntimeError> {
        let value = self.force_value(value)?;
        match value {
            Value::Null => Ok(false),
            Value::Boolean(b) => Ok(b),
            Value::Number(n) => Ok(n > 0.0),
            Value::String(s) => Ok(self.memory_manager.load_string(s) != ""),
            Value::Object(x) => Ok(self.memory_manager.load_object(x).len() > 0),
            Value::Array(x) => Ok(self.memory_manager.load_array(x).len() > 0),
            Value::Binary(x) => Ok(self.memory_manager.load_binary(x).data.len() > 0),
            Value::Function(_) => Ok(true), // Functions are truthy
            Value::Closure(_) => Ok(true),  // Closures are truthy
            Value::NativeFunction(_) => Ok(true), // Native functions are truthy
            Value::Import(_) => Ok(true),   // Should be unreachable due to force_value
        }
    }

    fn to_number(&mut self, value: Value) -> Result<f64, RuntimeError> {
        let value = self.force_value(value)?;
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

    fn to_integer(&mut self, value: Value) -> Result<i64, RuntimeError> {
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
        let mut vm = VirtualMachine::new(chunk, memory_manager);

        assert!(!vm.is_truthy(Value::Null).unwrap());
        assert!(!vm.is_truthy(Value::Boolean(false)).unwrap());
        assert!(vm.is_truthy(Value::Boolean(true)).unwrap());
        assert!(!vm.is_truthy(Value::Number(0.0)).unwrap());
        assert!(vm.is_truthy(Value::Number(1.0)).unwrap());
        assert!(!vm.is_truthy(Value::Number(-1.0)).unwrap());
        assert!(vm.is_truthy(Value::Number(0.1)).unwrap());

        let bin_empty = vm.memory_manager.allocate_binary(vec![]).index;
        assert!(!vm.is_truthy(Value::Binary(bin_empty)).unwrap());

        let bin_full = vm.memory_manager.allocate_binary(vec![1]).index;
        assert!(vm.is_truthy(Value::Binary(bin_full)).unwrap());
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
