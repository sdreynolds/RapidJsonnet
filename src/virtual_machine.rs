use chunk::{
    Chunk, ClosureIndex, FieldVisibility, I32_SIZE_BYTES, OPCODE_SIZE_BYTES, ObjectIndex, Opcode,
    OwnedChunk, RuntimeError, StringIndex, UpvalueIndex, Value,
};
use compiler;
use memory_manager::{MemoryManager, ObjectField};
use scanner;
use std::ops::Range;

use native::{self, call_native};

extern crate serde_yaml;

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
    /// External variables set via --ext-str / --ext-code CLI flags
    pub ext_vars: std::collections::HashMap<String, Value>,
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
            ext_vars: std::collections::HashMap::new(),
        }
    }

    /// Set an external variable as a string value (for --ext-str CLI flag)
    pub fn set_ext_var_string(&mut self, key: &str, value: &str) {
        let alloc = self.memory_manager.allocate_string(value);
        self.ext_vars
            .insert(key.to_string(), Value::String(alloc.index));
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

    /// Call a Value (Closure or NativeFunction) with two arguments and return its result.
    fn call_value_with_two_args(
        &mut self,
        func: Value,
        arg1: Value,
        arg2: Value,
    ) -> Result<Value, RuntimeError> {
        match func {
            Value::Closure(closure_index) => {
                self.push(func)?;
                self.push(arg1)?;
                self.push(arg2)?;
                let target_frame_count = self.frame_count;
                self.call_closure(closure_index, 2, None, None)?;
                self.interpret_until(target_frame_count)
            }
            Value::NativeFunction(id) => {
                let span = self.get_current_span();
                let source_id = self.current_chunk().source_id.to_string();
                call_native(id, &[arg1, arg2], &mut self.memory_manager, span, source_id)
            }
            _ => Err(RuntimeError {
                span: self.get_current_span(),
                message: "expected function as callback".to_string(),
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
                    let result = self.values_equal(&a, &b)?;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ne => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = !self.values_equal(&a, &b)?;
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

                    // Handle std.minArray / std.maxArray with optional keyF and onEmpty
                    if matches!(
                        func_id,
                        chunk::NativeFuncId::MinArray | chunk::NativeFuncId::MaxArray
                    ) && args.len() >= 2
                    {
                        let arr_val = args[0];
                        let key_f = args.get(1).copied();
                        let on_empty = args.get(2).copied();

                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.{} expected array, got {}",
                                        func_id.name(),
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };

                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();

                        if elements.is_empty() {
                            match on_empty {
                                Some(v) => {
                                    self.push(v)?;
                                    continue;
                                }
                                None => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!("std.{}: empty array", func_id.name()),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            }
                        }

                        // Check if keyF is null or absent
                        let effective_key_f = match key_f {
                            None | Some(Value::Null) => None,
                            Some(v) => Some(v),
                        };

                        if let Some(key_f_val) = effective_key_f {
                            // keyF provided — apply it to each element, compare keys
                            let mut best_elem = elements[0];
                            let mut best_key = {
                                let mut roots = Vec::from(self.stack.clone());
                                roots.extend_from_slice(&elements);
                                roots.push(key_f_val);
                                let mut open_upvalue_roots = Vec::new();
                                let mut upvalue = self.open_upvalues;
                                while let Some(uv_idx) = upvalue {
                                    open_upvalue_roots.push(uv_idx);
                                    upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                }
                                self.memory_manager
                                    .push_external_roots(roots, open_upvalue_roots);
                                let k = self.call_value_with_one_arg(key_f_val, elements[0]);
                                self.memory_manager.pop_external_roots();
                                k?
                            };

                            for &elem in elements.iter().skip(1) {
                                let key = {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f_val);
                                    roots.push(best_elem);
                                    roots.push(best_key);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let k = self.call_value_with_one_arg(key_f_val, elem);
                                    self.memory_manager.pop_external_roots();
                                    k?
                                };
                                let ord =
                                    native::compare_values(key, best_key, &self.memory_manager);
                                let take = if func_id == chunk::NativeFuncId::MinArray {
                                    ord == std::cmp::Ordering::Less
                                } else {
                                    ord == std::cmp::Ordering::Greater
                                };
                                if take {
                                    best_key = key;
                                    best_elem = elem;
                                }
                            }
                            self.push(best_elem)?;
                            continue;
                        } else {
                            // No keyF — compare elements directly
                            let mut best = elements[0];
                            for &elem in elements.iter().skip(1) {
                                let ord = native::compare_values(elem, best, &self.memory_manager);
                                let take = if func_id == chunk::NativeFuncId::MinArray {
                                    ord == std::cmp::Ordering::Less
                                } else {
                                    ord == std::cmp::Ordering::Greater
                                };
                                if take {
                                    best = elem;
                                }
                            }
                            self.push(best)?;
                            continue;
                        }
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
                        || func_id == chunk::NativeFuncId::ObjectKeysValues
                        || func_id == chunk::NativeFuncId::ObjectKeysValuesAll)
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

                    // Handle manifestJson* variants in the VM
                    if matches!(
                        func_id,
                        chunk::NativeFuncId::ManifestJson
                            | chunk::NativeFuncId::ManifestJsonMinified
                            | chunk::NativeFuncId::ManifestJsonEx
                    ) {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let (indent, newline, kvs) = match func_id {
                            chunk::NativeFuncId::ManifestJson => {
                                ("   ".to_string(), "\n".to_string(), ": ".to_string())
                            }
                            chunk::NativeFuncId::ManifestJsonMinified => {
                                ("".to_string(), "".to_string(), ":".to_string())
                            }
                            chunk::NativeFuncId::ManifestJsonEx => {
                                let i = match args[1] {
                                    Value::String(s_idx) => {
                                        self.memory_manager.load_string(s_idx).to_string()
                                    }
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "std.manifestJsonEx: indent must be a string"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let n = match args[2] {
                                    Value::String(s_idx) => {
                                        self.memory_manager.load_string(s_idx).to_string()
                                    }
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "std.manifestJsonEx: newline must be a string"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let k =
                                    match args[3] {
                                        Value::String(s_idx) => {
                                            self.memory_manager.load_string(s_idx).to_string()
                                        }
                                        _ => return Err(RuntimeError {
                                            span,
                                            message:
                                                "std.manifestJsonEx: key_val_sep must be a string"
                                                    .to_string(),
                                            source_id,
                                        }),
                                    };
                                (i, n, k)
                            }
                            _ => unreachable!(),
                        };
                        let value = args[0];
                        let json = self.manifest_json_value(
                            value,
                            &indent,
                            &newline,
                            &kvs,
                            0,
                            span.clone(),
                            &source_id,
                        )?;
                        let idx = self.memory_manager.allocate_string(&json);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle std.map
                    if func_id == chunk::NativeFuncId::Map {
                        let func_val = args[0];
                        let arr_val = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.map: second argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results = Vec::with_capacity(elements.len());
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(func_val);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result = self.call_value_with_one_arg(func_val, elem);
                            self.memory_manager.pop_external_roots();
                            results.push(result?);
                        }
                        let alloc = self.memory_manager.allocate_array(results);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.filter
                    if func_id == chunk::NativeFuncId::Filter {
                        let func_val = args[0];
                        let arr_val = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.filter: second argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results = Vec::new();
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(func_val);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result = self.call_value_with_one_arg(func_val, elem);
                            self.memory_manager.pop_external_roots();
                            match result? {
                                Value::Boolean(true) => results.push(elem),
                                Value::Boolean(false) => {}
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.filter: function must return boolean, got {:?}",
                                            other
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            }
                        }
                        let alloc = self.memory_manager.allocate_array(results);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.foldl
                    if func_id == chunk::NativeFuncId::Foldl {
                        let func_val = args[0];
                        let arr_val = args[1];
                        let mut acc = args[2];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.foldl: second argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(func_val);
                            roots.push(acc);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result = self.call_value_with_two_args(func_val, acc, elem);
                            self.memory_manager.pop_external_roots();
                            acc = result?;
                        }
                        self.push(acc)?;
                        continue;
                    }

                    // Handle std.flatMap
                    if func_id == chunk::NativeFuncId::FlatMap {
                        let func_val = args[0];
                        match args[1] {
                            Value::Array(arr_idx) => {
                                let elements =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let mut results = Vec::new();
                                for &elem in &elements {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.extend_from_slice(&results);
                                    roots.push(func_val);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let sub = self.call_value_with_one_arg(func_val, elem);
                                    self.memory_manager.pop_external_roots();
                                    match sub? {
                                        Value::Array(sub_idx) => {
                                            let sub_elems = self
                                                .memory_manager
                                                .load_array(sub_idx)
                                                .elements
                                                .clone();
                                            results.extend(sub_elems);
                                        }
                                        _ => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message:
                                                    "std.flatMap: function must return array for array input"
                                                        .to_string(),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    }
                                }
                                let alloc = self.memory_manager.allocate_array(results);
                                self.push(Value::Array(alloc.index))?;
                            }
                            Value::String(s_idx) => {
                                let s = self.memory_manager.load_string(s_idx).to_string();
                                let chars: Vec<Value> = s
                                    .chars()
                                    .map(|c| {
                                        let alloc =
                                            self.memory_manager.allocate_string(&c.to_string());
                                        Value::String(alloc.index)
                                    })
                                    .collect();
                                let mut out = String::new();
                                for &char_val in &chars {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&chars);
                                    roots.push(func_val);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let result = self.call_value_with_one_arg(func_val, char_val);
                                    self.memory_manager.pop_external_roots();
                                    match result? {
                                        Value::String(rs) => {
                                            out.push_str(self.memory_manager.load_string(rs))
                                        }
                                        _ => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message:
                                                    "std.flatMap: function must return string for string input"
                                                        .to_string(),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    }
                                }
                                let alloc = self.memory_manager.allocate_string(&out);
                                self.push(Value::String(alloc.index))?;
                            }
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.flatMap: second argument must be array or string"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        }
                        continue;
                    }

                    // Handle std.mapWithIndex
                    if func_id == chunk::NativeFuncId::MapWithIndex {
                        let func_val = args[0];
                        let arr_idx = match args[1] {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.mapWithIndex: second argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results = Vec::with_capacity(elements.len());
                        for (i, &elem) in elements.iter().enumerate() {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(func_val);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result = self.call_value_with_two_args(
                                func_val,
                                Value::Number(i as f64),
                                elem,
                            );
                            self.memory_manager.pop_external_roots();
                            results.push(result?);
                        }
                        let alloc = self.memory_manager.allocate_array(results);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.foldr (right-to-left fold, element first argument)
                    if func_id == chunk::NativeFuncId::Foldr {
                        let func_val = args[0];
                        let arr_val = args[1];
                        let mut acc = args[2];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.foldr: second argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        for &elem in elements.iter().rev() {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(func_val);
                            roots.push(acc);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result = self.call_value_with_two_args(func_val, elem, acc);
                            self.memory_manager.pop_external_roots();
                            acc = result?;
                        }
                        self.push(acc)?;
                        continue;
                    }

                    // Handle std.mapWithKey (2-arg callback over object fields: key, value)
                    if func_id == chunk::NativeFuncId::MapWithKey {
                        let func_val = args[0];
                        let obj_val = args[1];
                        let o_idx = match obj_val {
                            Value::Object(o) => o,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.mapWithKey: second argument must be an object"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        // Collect visible fields: (key StringIndex, raw value, visibility)
                        let field_data: Vec<(StringIndex, Value, FieldVisibility)> = {
                            let obj = self.memory_manager.load_object(o_idx);
                            obj.properties
                                .iter()
                                .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                .map(|(k, f)| (*k, f.value, f.visibility))
                                .collect()
                        };
                        let mut new_properties: std::collections::HashMap<
                            StringIndex,
                            ObjectField,
                        > = std::collections::HashMap::new();
                        for (k_idx, raw_val, vis) in &field_data {
                            let k_idx = *k_idx;
                            let raw_val = *raw_val;
                            let vis = *vis;
                            // Force-evaluate thunk if needed
                            let evaled_val = match raw_val {
                                Value::Closure(closure_idx) => {
                                    self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                }
                                other => other,
                            };
                            // Build key string value
                            let key_str = self.memory_manager.load_string(k_idx).to_string();
                            let key_alloc = self.memory_manager.allocate_string(&key_str);
                            let key_val = Value::String(key_alloc.index);
                            // GC root
                            let mut roots = Vec::from(self.stack.clone());
                            roots.push(func_val);
                            roots.push(evaled_val);
                            roots.push(key_val);
                            for f in new_properties.values() {
                                roots.push(f.value);
                            }
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let result =
                                self.call_value_with_two_args(func_val, key_val, evaled_val);
                            self.memory_manager.pop_external_roots();
                            new_properties.insert(
                                k_idx,
                                ObjectField {
                                    value: result?,
                                    super_obj: None,
                                    visibility: vis,
                                },
                            );
                        }
                        let alloc = self
                            .memory_manager
                            .allocate_object_with_properties(new_properties);
                        self.push(Value::Object(alloc.index))?;
                        continue;
                    }

                    // Handle std.filterMap (filter then map, both 1-arg callbacks)
                    if func_id == chunk::NativeFuncId::FilterMap {
                        let filter_func = args[0];
                        let map_func = args[1];
                        let arr_val = args[2];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: "std.filterMap: third argument must be an array"
                                        .to_string(),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results: Vec<Value> = Vec::new();
                        for &elem in &elements {
                            // Filter pass
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(filter_func);
                            roots.push(map_func);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let keep = self.call_value_with_one_arg(filter_func, elem);
                            self.memory_manager.pop_external_roots();
                            match keep? {
                                Value::Boolean(true) => {
                                    // Map pass
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.extend_from_slice(&results);
                                    roots.push(filter_func);
                                    roots.push(map_func);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let mapped = self.call_value_with_one_arg(map_func, elem);
                                    self.memory_manager.pop_external_roots();
                                    results.push(mapped?);
                                }
                                Value::Boolean(false) => {}
                                _ => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message:
                                            "std.filterMap: filter function must return a boolean"
                                                .to_string(),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            }
                        }
                        let alloc = self.memory_manager.allocate_array(results);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.extVar
                    if func_id == chunk::NativeFuncId::ExtVar {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let key = match args[0] {
                            Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                            other => {
                                return Err(RuntimeError {
                                    span,
                                    message: format!(
                                        "std.extVar: argument must be a string, got {}",
                                        other.type_name()
                                    ),
                                    source_id,
                                });
                            }
                        };
                        match self.ext_vars.get(&key).copied() {
                            Some(val) => {
                                self.push(val)?;
                                continue;
                            }
                            None => {
                                return Err(RuntimeError {
                                    span,
                                    message: format!("Undefined external variable: '{}'", key),
                                    source_id,
                                });
                            }
                        }
                    }

                    // Handle set operations with keyF (3-arg forms)
                    if func_id == chunk::NativeFuncId::SetInter && args.len() == 3 {
                        let a_val = args[0];
                        let b_val = args[1];
                        let key_f = args[2];
                        let a_idx = match a_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setInter: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setInter: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let a_elems: Vec<Value> =
                            self.memory_manager.load_array(a_idx).elements.clone();
                        let b_elems: Vec<Value> =
                            self.memory_manager.load_array(b_idx).elements.clone();
                        let mut a_keys: Vec<Value> = Vec::with_capacity(a_elems.len());
                        for &elem in &a_elems {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&a_elems);
                            roots.extend_from_slice(&b_elems);
                            roots.extend_from_slice(&a_keys);
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
                            a_keys.push(key?);
                        }
                        let mut b_keys: Vec<Value> = Vec::with_capacity(b_elems.len());
                        for &elem in &b_elems {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&a_elems);
                            roots.extend_from_slice(&b_elems);
                            roots.extend_from_slice(&a_keys);
                            roots.extend_from_slice(&b_keys);
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
                            b_keys.push(key?);
                        }
                        let mut result = Vec::new();
                        let (mut i, mut j) = (0usize, 0usize);
                        while i < a_elems.len() && j < b_elems.len() {
                            let cmp =
                                native::compare_values(a_keys[i], b_keys[j], &self.memory_manager);
                            match cmp {
                                std::cmp::Ordering::Less => i += 1,
                                std::cmp::Ordering::Greater => j += 1,
                                std::cmp::Ordering::Equal => {
                                    result.push(a_elems[i]);
                                    i += 1;
                                    j += 1;
                                }
                            }
                        }
                        let alloc = self.memory_manager.allocate_array(result);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    if func_id == chunk::NativeFuncId::SetDiff && args.len() == 3 {
                        let a_val = args[0];
                        let b_val = args[1];
                        let key_f = args[2];
                        let a_idx = match a_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setDiff: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setDiff: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let a_elems: Vec<Value> =
                            self.memory_manager.load_array(a_idx).elements.clone();
                        let b_elems: Vec<Value> =
                            self.memory_manager.load_array(b_idx).elements.clone();
                        let mut a_keys: Vec<Value> = Vec::with_capacity(a_elems.len());
                        for &elem in &a_elems {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&a_elems);
                            roots.extend_from_slice(&b_elems);
                            roots.extend_from_slice(&a_keys);
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
                            a_keys.push(key?);
                        }
                        let mut b_keys: Vec<Value> = Vec::with_capacity(b_elems.len());
                        for &elem in &b_elems {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&a_elems);
                            roots.extend_from_slice(&b_elems);
                            roots.extend_from_slice(&a_keys);
                            roots.extend_from_slice(&b_keys);
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
                            b_keys.push(key?);
                        }
                        let mut result = Vec::new();
                        let (mut i, mut j) = (0usize, 0usize);
                        while i < a_elems.len() {
                            if j >= b_elems.len() {
                                result.push(a_elems[i]);
                                i += 1;
                                continue;
                            }
                            let cmp =
                                native::compare_values(a_keys[i], b_keys[j], &self.memory_manager);
                            match cmp {
                                std::cmp::Ordering::Less => {
                                    result.push(a_elems[i]);
                                    i += 1;
                                }
                                std::cmp::Ordering::Greater => {
                                    j += 1;
                                }
                                std::cmp::Ordering::Equal => {
                                    i += 1;
                                    j += 1;
                                }
                            }
                        }
                        let alloc = self.memory_manager.allocate_array(result);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    if func_id == chunk::NativeFuncId::SetMember && args.len() == 3 {
                        let x_val = args[0];
                        let arr_val = args[1];
                        let key_f = args[2];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setMember: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let arr_elems: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        // Compute key for x
                        let x_key = {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&arr_elems);
                            roots.push(key_f);
                            roots.push(x_val);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let k = self.call_value_with_one_arg(key_f, x_val);
                            self.memory_manager.pop_external_roots();
                            k?
                        };
                        // Binary search
                        let mut lo = 0usize;
                        let mut hi = arr_elems.len();
                        let mut found = false;
                        while lo < hi {
                            let mid = lo + (hi - lo) / 2;
                            let mid_key = {
                                let mut roots = Vec::from(self.stack.clone());
                                roots.extend_from_slice(&arr_elems);
                                roots.push(key_f);
                                roots.push(x_val);
                                roots.push(x_key);
                                let mut open_upvalue_roots = Vec::new();
                                let mut upvalue = self.open_upvalues;
                                while let Some(uv_idx) = upvalue {
                                    open_upvalue_roots.push(uv_idx);
                                    upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                }
                                self.memory_manager
                                    .push_external_roots(roots, open_upvalue_roots);
                                let k = self.call_value_with_one_arg(key_f, arr_elems[mid]);
                                self.memory_manager.pop_external_roots();
                                k?
                            };
                            let cmp = native::compare_values(x_key, mid_key, &self.memory_manager);
                            match cmp {
                                std::cmp::Ordering::Equal => {
                                    found = true;
                                    break;
                                }
                                std::cmp::Ordering::Less => hi = mid,
                                std::cmp::Ordering::Greater => lo = mid + 1,
                            }
                        }
                        self.push(Value::Boolean(found))?;
                        continue;
                    }

                    if func_id == chunk::NativeFuncId::SetUnion && args.len() == 3 {
                        let a_val = args[0];
                        let b_val = args[1];
                        let key_f = args[2];
                        let a_idx = match a_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setUnion: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "setUnion: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let mut combined = self.memory_manager.load_array(a_idx).elements.clone();
                        combined.extend_from_slice(
                            &self.memory_manager.load_array(b_idx).elements.clone(),
                        );
                        // Sort combined by keyF
                        let mut keys: Vec<Value> = Vec::with_capacity(combined.len());
                        for &elem in &combined {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&combined);
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
                        let mut indexed: Vec<usize> = (0..combined.len()).collect();
                        {
                            let mm = &self.memory_manager;
                            indexed.sort_by(|&a, &b| native::compare_values(keys[a], keys[b], mm));
                        }
                        let sorted_elems: Vec<Value> =
                            indexed.iter().map(|&i| combined[i]).collect();
                        let sorted_keys: Vec<Value> = indexed.iter().map(|&i| keys[i]).collect();
                        // Dedup by keyF: keep first occurrence of each key
                        let mut result: Vec<Value> = Vec::new();
                        let mut last_key: Option<Value> = None;
                        for (elem, key) in sorted_elems.iter().zip(sorted_keys.iter()) {
                            if let Some(lk) = last_key {
                                if native::compare_values(lk, *key, &self.memory_manager)
                                    == std::cmp::Ordering::Equal
                                {
                                    continue;
                                }
                            }
                            result.push(*elem);
                            last_key = Some(*key);
                        }
                        let alloc = self.memory_manager.allocate_array(result);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.parseJson
                    if func_id == chunk::NativeFuncId::ParseJson {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let s_idx = match args[0] {
                            Value::String(s) => s,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "std.parseJson: argument must be a string".to_string(),
                                    source_id,
                                });
                            }
                        };
                        let s = self.memory_manager.load_string(s_idx).to_string();
                        let parsed: serde_json::Value =
                            serde_json::from_str(&s).map_err(|e| RuntimeError {
                                span: span.clone(),
                                message: format!("std.parseJson: {}", e),
                                source_id: source_id.clone(),
                            })?;
                        let result = self.json_to_jsonnet_value(&parsed)?;
                        self.push(result)?;
                        continue;
                    }

                    if matches!(
                        func_id,
                        chunk::NativeFuncId::MergePatch
                            | chunk::NativeFuncId::Prune
                            | chunk::NativeFuncId::Uniq
                            | chunk::NativeFuncId::Set
                            | chunk::NativeFuncId::SetUnion
                    ) {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();

                        match func_id {
                            chunk::NativeFuncId::MergePatch => {
                                let result = self.merge_patch_value(args[0], args[1])?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::Prune => {
                                let result = self.prune_value(args[0])?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::Uniq => {
                                let result = self.uniq_value(args[0], args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::Set => {
                                let arr_val = args[0];
                                let sorted = crate::native::call_native(
                                    chunk::NativeFuncId::Sort,
                                    &[arr_val],
                                    &mut self.memory_manager,
                                    span.clone(),
                                    source_id.clone(),
                                )?;
                                let result = self.uniq_value(sorted, args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::SetUnion => {
                                let a_val = args[0];
                                let b_val = args[1];
                                let a_idx = match a_val {
                                    Value::Array(i) => i,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message:
                                                "std.setUnion: first argument must be an array"
                                                    .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let b_idx = match b_val {
                                    Value::Array(i) => i,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message:
                                                "std.setUnion: second argument must be an array"
                                                    .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let mut combined =
                                    self.memory_manager.load_array(a_idx).elements.clone();
                                combined.extend_from_slice(
                                    &self.memory_manager.load_array(b_idx).elements.clone(),
                                );
                                let alloc = self.memory_manager.allocate_array(combined);
                                let sorted = crate::native::call_native(
                                    chunk::NativeFuncId::Sort,
                                    &[Value::Array(alloc.index)],
                                    &mut self.memory_manager,
                                    span.clone(),
                                    source_id.clone(),
                                )?;
                                let result = self.uniq_value(sorted, args.get(2).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            _ => unreachable!(),
                        }
                    }

                    // Handle manifestIni
                    if func_id == chunk::NativeFuncId::ManifestIni {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let result = self.manifest_ini(value, span, source_id)?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle manifestPython
                    if func_id == chunk::NativeFuncId::ManifestPython {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let result = self.manifest_python_value(value, 0, span, source_id)?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle manifestPythonVars
                    if func_id == chunk::NativeFuncId::ManifestPythonVars {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let result = self.manifest_python_vars(value, span, source_id)?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle manifestYamlDoc
                    if func_id == chunk::NativeFuncId::ManifestYamlDoc {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let indent_array_in_object = match args[1] {
                            Value::Boolean(b) => b,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "manifestYamlDoc: indent_array_in_object must be bool"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let quote_keys = match args[2] {
                            Value::Boolean(b) => b,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "manifestYamlDoc: quote_keys must be bool".to_string(),
                                    source_id,
                                });
                            }
                        };
                        let result = self.manifest_yaml_doc(
                            value,
                            0,
                            indent_array_in_object,
                            quote_keys,
                            span,
                            source_id,
                        )?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle manifestYamlStream
                    if func_id == chunk::NativeFuncId::ManifestYamlStream {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let indent_array_in_object = match args[1] {
                            Value::Boolean(b) => b,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message:
                                        "manifestYamlStream: indent_array_in_object must be bool"
                                            .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let c_document_end = match args[2] {
                            Value::Boolean(b) => b,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "manifestYamlStream: c_document_end must be bool"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let quote_keys = match args[3] {
                            Value::Boolean(b) => b,
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "manifestYamlStream: quote_keys must be bool"
                                        .to_string(),
                                    source_id,
                                });
                            }
                        };
                        let result = self.manifest_yaml_stream(
                            value,
                            indent_array_in_object,
                            c_document_end,
                            quote_keys,
                            span,
                            source_id,
                        )?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle parseYaml
                    if func_id == chunk::NativeFuncId::ParseYaml {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let s = match args[0] {
                            Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "parseYaml: argument must be a string".to_string(),
                                    source_id,
                                });
                            }
                        };
                        let yaml_val: serde_yaml::Value =
                            serde_yaml::from_str(&s).map_err(|e| RuntimeError {
                                span: span.clone(),
                                message: format!("parseYaml: {}", e),
                                source_id: source_id.clone(),
                            })?;
                        let result = self.serde_yaml_to_jsonnet_value(yaml_val, span, source_id)?;
                        self.push(result)?;
                        continue;
                    }

                    // Handle manifestXmlJsonml
                    if func_id == chunk::NativeFuncId::ManifestXmlJsonml {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let result = self.manifest_xml_jsonml(value, span, source_id)?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle manifestTomlEx
                    if func_id == chunk::NativeFuncId::ManifestTomlEx {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let value = args[0];
                        let indent = match args[1] {
                            Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                            _ => {
                                return Err(RuntimeError {
                                    span,
                                    message: "manifestTomlEx: indent must be a string".to_string(),
                                    source_id,
                                });
                            }
                        };
                        let result =
                            self.manifest_toml_ex(value, &indent.clone(), span, source_id)?;
                        let idx = self.memory_manager.allocate_string(&result);
                        self.push(Value::String(idx.index))?;
                        continue;
                    }

                    // Handle std.groupBy
                    if func_id == chunk::NativeFuncId::GroupBy {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.groupBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut group_order: Vec<String> = Vec::new();
                        let mut groups: std::collections::HashMap<String, Vec<Value>> =
                            std::collections::HashMap::new();
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(key_f);
                            for group_elems in groups.values() {
                                roots.extend_from_slice(group_elems);
                            }
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let key_val = self.call_value_with_one_arg(key_f, elem);
                            self.memory_manager.pop_external_roots();
                            let key_val = key_val?;
                            let key_str = match key_val {
                                Value::String(idx) => {
                                    self.memory_manager.load_string(idx).to_string()
                                }
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.groupBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            };
                            if !groups.contains_key(&key_str) {
                                group_order.push(key_str.clone());
                                groups.insert(key_str.clone(), Vec::new());
                            }
                            groups.get_mut(&key_str).unwrap().push(elem);
                        }
                        let mut properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();
                        for key_str in &group_order {
                            let group_elems = groups.remove(key_str).unwrap_or_default();
                            let arr_alloc = self.memory_manager.allocate_array(group_elems);
                            let k_alloc = self.memory_manager.allocate_string(key_str);
                            properties.insert(
                                k_alloc.index,
                                memory_manager::ObjectField {
                                    value: Value::Array(arr_alloc.index),
                                    super_obj: None,
                                    visibility: FieldVisibility::Visible,
                                },
                            );
                        }
                        let obj_alloc = self
                            .memory_manager
                            .allocate_object_with_properties(properties);
                        self.push(Value::Object(obj_alloc.index))?;
                        continue;
                    }

                    // Handle std.sortBy
                    if func_id == chunk::NativeFuncId::SortBy {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.sortBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        // Phase 1: pre-compute keys for all elements
                        let mut keyed: Vec<(Value, Value)> = Vec::with_capacity(elements.len());
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(key_f);
                            for (k, v) in &keyed {
                                roots.push(*k);
                                roots.push(*v);
                            }
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
                            keyed.push((key?, elem));
                        }
                        // Phase 2: sort by pre-computed keys
                        keyed.sort_by(|(ka, _), (kb, _)| {
                            native::compare_values(*ka, *kb, &self.memory_manager)
                        });
                        // Phase 3: extract sorted elements
                        let sorted: Vec<Value> = keyed.into_iter().map(|(_, v)| v).collect();
                        let alloc = self.memory_manager.allocate_array(sorted);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.countBy
                    if func_id == chunk::NativeFuncId::CountBy {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.countBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut group_order: Vec<String> = Vec::new();
                        let mut counts: std::collections::HashMap<String, u64> =
                            std::collections::HashMap::new();
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(key_f);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let key_val = self.call_value_with_one_arg(key_f, elem);
                            self.memory_manager.pop_external_roots();
                            let key_val = key_val?;
                            let key_str = match key_val {
                                Value::String(idx) => {
                                    self.memory_manager.load_string(idx).to_string()
                                }
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.countBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            };
                            if !counts.contains_key(&key_str) {
                                group_order.push(key_str.clone());
                            }
                            *counts.entry(key_str).or_insert(0) += 1;
                        }
                        let mut properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();
                        for key_str in &group_order {
                            let count = counts[key_str];
                            let k_alloc = self.memory_manager.allocate_string(key_str);
                            properties.insert(
                                k_alloc.index,
                                memory_manager::ObjectField {
                                    value: Value::Number(count as f64),
                                    super_obj: None,
                                    visibility: FieldVisibility::Visible,
                                },
                            );
                        }
                        let obj_alloc = self
                            .memory_manager
                            .allocate_object_with_properties(properties);
                        self.push(Value::Object(obj_alloc.index))?;
                        continue;
                    }

                    // Handle std.uniqBy
                    if func_id == chunk::NativeFuncId::UniqBy {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.uniqBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut seen: std::collections::HashSet<String> =
                            std::collections::HashSet::new();
                        let mut result: Vec<Value> = Vec::new();
                        for &elem in &elements {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&result);
                            roots.push(key_f);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let key_val = self.call_value_with_one_arg(key_f, elem);
                            self.memory_manager.pop_external_roots();
                            let key_val = key_val?;
                            let key_str = match key_val {
                                Value::String(idx) => {
                                    self.memory_manager.load_string(idx).to_string()
                                }
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.uniqBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            };
                            if seen.insert(key_str) {
                                result.push(elem);
                            }
                        }
                        let alloc = self.memory_manager.allocate_array(result);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.minBy and std.maxBy
                    if func_id == chunk::NativeFuncId::MinBy
                        || func_id == chunk::NativeFuncId::MaxBy
                    {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.{}: expected array, got {}",
                                        func_id.name(),
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        if elements.is_empty() {
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("std.{}: array must not be empty", func_id.name()),
                                source_id: self.current_chunk().source_id.to_string(),
                            });
                        }
                        // Compute key for first element
                        let (mut best_key, mut best_elem) = {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(key_f);
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let k = self.call_value_with_one_arg(key_f, elements[0]);
                            self.memory_manager.pop_external_roots();
                            (k?, elements[0])
                        };
                        for &elem in elements.iter().skip(1) {
                            let mut roots = Vec::from(self.stack.clone());
                            roots.extend_from_slice(&elements);
                            roots.push(key_f);
                            roots.push(best_key);
                            roots.push(best_elem);
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
                            let cmp = native::compare_values(key, best_key, &self.memory_manager);
                            let take = if func_id == chunk::NativeFuncId::MinBy {
                                cmp == std::cmp::Ordering::Less
                            } else {
                                cmp == std::cmp::Ordering::Greater
                            };
                            if take {
                                best_key = key;
                                best_elem = elem;
                            }
                        }
                        self.push(best_elem)?;
                        continue;
                    }

                    // Handle std.toPairs
                    if func_id == chunk::NativeFuncId::ToPairs {
                        let obj_val = args[0];
                        let o_idx = match obj_val {
                            Value::Object(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.toPairs: expected object, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value)> = {
                            let obj = self.memory_manager.load_object(o_idx);
                            obj.properties
                                .iter()
                                .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                .map(|(k, f)| (*k, f.value))
                                .collect()
                        };
                        let mut pairs: Vec<Value> = Vec::with_capacity(field_data.len());
                        for (k_idx, raw_val) in &field_data {
                            let k_idx = *k_idx;
                            let raw_val = *raw_val;
                            let evaled_val = match raw_val {
                                Value::Closure(closure_idx) => {
                                    self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                }
                                other => other,
                            };
                            let key_str = self.memory_manager.load_string(k_idx).to_string();
                            let k_alloc = self.memory_manager.allocate_string(&key_str);
                            let k_val = Value::String(k_alloc.index);
                            let pair_alloc =
                                self.memory_manager.allocate_array(vec![k_val, evaled_val]);
                            pairs.push(Value::Array(pair_alloc.index));
                        }
                        let alloc = self.memory_manager.allocate_array(pairs);
                        self.push(Value::Array(alloc.index))?;
                        continue;
                    }

                    // Handle std.mapKeys
                    if func_id == chunk::NativeFuncId::MapKeys {
                        let func_val = args[0];
                        let obj_val = args[1];
                        let o_idx = match obj_val {
                            Value::Object(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.mapKeys: expected object, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = {
                            let obj = self.memory_manager.load_object(o_idx);
                            obj.properties
                                .iter()
                                .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                .map(|(k, f)| (*k, f.value, f.visibility))
                                .collect()
                        };
                        let mut new_properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();
                        for (k_idx, raw_val, vis) in &field_data {
                            let k_idx = *k_idx;
                            let raw_val = *raw_val;
                            let vis = *vis;
                            let evaled_val = match raw_val {
                                Value::Closure(closure_idx) => {
                                    self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                }
                                other => other,
                            };
                            let key_str = self.memory_manager.load_string(k_idx).to_string();
                            let key_alloc = self.memory_manager.allocate_string(&key_str);
                            let key_val = Value::String(key_alloc.index);
                            let mut roots = Vec::from(self.stack.clone());
                            roots.push(func_val);
                            roots.push(evaled_val);
                            roots.push(key_val);
                            for f in new_properties.values() {
                                roots.push(f.value);
                            }
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let new_key_val = self.call_value_with_one_arg(func_val, key_val);
                            self.memory_manager.pop_external_roots();
                            let new_key_val = new_key_val?;
                            let new_key_str = match new_key_val {
                                Value::String(idx) => {
                                    self.memory_manager.load_string(idx).to_string()
                                }
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.mapKeys: func must return string, got {}",
                                            other.type_name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            };
                            let new_k_alloc = self.memory_manager.allocate_string(&new_key_str);
                            new_properties.insert(
                                new_k_alloc.index,
                                memory_manager::ObjectField {
                                    value: evaled_val,
                                    super_obj: None,
                                    visibility: vis,
                                },
                            );
                        }
                        let alloc = self
                            .memory_manager
                            .allocate_object_with_properties(new_properties);
                        self.push(Value::Object(alloc.index))?;
                        continue;
                    }

                    // Handle std.filterObject
                    if func_id == chunk::NativeFuncId::FilterObject {
                        let func_val = args[0];
                        let obj_val = args[1];
                        let o_idx = match obj_val {
                            Value::Object(i) => i,
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.filterObject: expected object, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = {
                            let obj = self.memory_manager.load_object(o_idx);
                            obj.properties
                                .iter()
                                .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                .map(|(k, f)| (*k, f.value, f.visibility))
                                .collect()
                        };
                        let mut kept_properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();
                        for (k_idx, raw_val, vis) in &field_data {
                            let k_idx = *k_idx;
                            let raw_val = *raw_val;
                            let vis = *vis;
                            let evaled_val = match raw_val {
                                Value::Closure(closure_idx) => {
                                    self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                }
                                other => other,
                            };
                            let key_str = self.memory_manager.load_string(k_idx).to_string();
                            let key_alloc = self.memory_manager.allocate_string(&key_str);
                            let key_val = Value::String(key_alloc.index);
                            let mut roots = Vec::from(self.stack.clone());
                            roots.push(func_val);
                            roots.push(evaled_val);
                            roots.push(key_val);
                            for f in kept_properties.values() {
                                roots.push(f.value);
                            }
                            let mut open_upvalue_roots = Vec::new();
                            let mut upvalue = self.open_upvalues;
                            while let Some(uv_idx) = upvalue {
                                open_upvalue_roots.push(uv_idx);
                                upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                            }
                            self.memory_manager
                                .push_external_roots(roots, open_upvalue_roots);
                            let keep = self.call_value_with_two_args(func_val, key_val, evaled_val);
                            self.memory_manager.pop_external_roots();
                            match keep? {
                                Value::Boolean(true) => {
                                    kept_properties.insert(
                                        k_idx,
                                        memory_manager::ObjectField {
                                            value: evaled_val,
                                            super_obj: None,
                                            visibility: vis,
                                        },
                                    );
                                }
                                Value::Boolean(false) => {}
                                other => {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.filterObject: func must return bool, got {}",
                                            other.type_name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                            }
                        }
                        let alloc = self
                            .memory_manager
                            .allocate_object_with_properties(kept_properties);
                        self.push(Value::Object(alloc.index))?;
                        continue;
                    }

                    // Handle std.objectFlatten
                    if func_id == chunk::NativeFuncId::ObjectFlatten {
                        let obj_val = args[0];
                        let sep_val = args[1];
                        let sep = match sep_val {
                            Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                            other => {
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!(
                                        "std.objectFlatten: sep must be string, got {}",
                                        other.type_name()
                                    ),
                                    source_id: self.current_chunk().source_id.to_string(),
                                });
                            }
                        };
                        let mut flat_fields: Vec<(String, Value)> = Vec::new();
                        self.flatten_object_recursive(
                            obj_val,
                            &sep,
                            String::new(),
                            &mut flat_fields,
                        )?;
                        let mut properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();
                        for (k_str, v) in flat_fields {
                            let k_alloc = self.memory_manager.allocate_string(&k_str);
                            properties.insert(
                                k_alloc.index,
                                memory_manager::ObjectField {
                                    value: v,
                                    super_obj: None,
                                    visibility: FieldVisibility::Visible,
                                },
                            );
                        }
                        let obj_alloc = self
                            .memory_manager
                            .allocate_object_with_properties(properties);
                        self.push(Value::Object(obj_alloc.index))?;
                        continue;
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

                            if matches!(
                                id,
                                chunk::NativeFuncId::MergePatch
                                    | chunk::NativeFuncId::Prune
                                    | chunk::NativeFuncId::Uniq
                                    | chunk::NativeFuncId::Set
                                    | chunk::NativeFuncId::SetUnion
                            ) {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();

                                match id {
                                    chunk::NativeFuncId::MergePatch => {
                                        let result = self.merge_patch_value(args[0], args[1])?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::Prune => {
                                        let result = self.prune_value(args[0])?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::Uniq => {
                                        let result =
                                            self.uniq_value(args[0], args.get(1).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::Set => {
                                        let arr_val = args[0];
                                        let sorted = crate::native::call_native(
                                            chunk::NativeFuncId::Sort,
                                            &[arr_val],
                                            &mut self.memory_manager,
                                            span.clone(),
                                            source_id.clone(),
                                        )?;
                                        let result =
                                            self.uniq_value(sorted, args.get(1).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::SetUnion => {
                                        let a_val = args[0];
                                        let b_val = args[1];
                                        let a_idx = match a_val {
                                            Value::Array(i) => i,
                                            _ => return Err(RuntimeError {
                                                span,
                                                message:
                                                    "std.setUnion: first argument must be an array"
                                                        .to_string(),
                                                source_id,
                                            }),
                                        };
                                        let b_idx = match b_val {
                                            Value::Array(i) => i,
                                            _ => return Err(RuntimeError {
                                                span,
                                                message:
                                                    "std.setUnion: second argument must be an array"
                                                        .to_string(),
                                                source_id,
                                            }),
                                        };
                                        let mut combined =
                                            self.memory_manager.load_array(a_idx).elements.clone();
                                        combined.extend_from_slice(
                                            &self.memory_manager.load_array(b_idx).elements.clone(),
                                        );
                                        let alloc = self.memory_manager.allocate_array(combined);
                                        let sorted = crate::native::call_native(
                                            chunk::NativeFuncId::Sort,
                                            &[Value::Array(alloc.index)],
                                            &mut self.memory_manager,
                                            span.clone(),
                                            source_id.clone(),
                                        )?;
                                        let result =
                                            self.uniq_value(sorted, args.get(2).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    _ => unreachable!(),
                                }
                            }

                            // Handle manifestIni
                            if id == chunk::NativeFuncId::ManifestIni {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let result = self.manifest_ini(value, span, source_id)?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle manifestPython
                            if id == chunk::NativeFuncId::ManifestPython {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let result =
                                    self.manifest_python_value(value, 0, span, source_id)?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle manifestPythonVars
                            if id == chunk::NativeFuncId::ManifestPythonVars {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let result = self.manifest_python_vars(value, span, source_id)?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle manifestYamlDoc
                            if id == chunk::NativeFuncId::ManifestYamlDoc {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let indent_array_in_object = match args[1] {
                                    Value::Boolean(b) => b,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message:
                                                "manifestYamlDoc: indent_array_in_object must be bool"
                                                    .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let quote_keys = match args[2] {
                                    Value::Boolean(b) => b,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "manifestYamlDoc: quote_keys must be bool"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let result = self.manifest_yaml_doc(
                                    value,
                                    0,
                                    indent_array_in_object,
                                    quote_keys,
                                    span,
                                    source_id,
                                )?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle manifestYamlStream
                            if id == chunk::NativeFuncId::ManifestYamlStream {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let indent_array_in_object = match args[1] {
                                    Value::Boolean(b) => b,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "manifestYamlStream: indent_array_in_object must be bool".to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let c_document_end = match args[2] {
                                    Value::Boolean(b) => b,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message:
                                                "manifestYamlStream: c_document_end must be bool"
                                                    .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let quote_keys = match args[3] {
                                    Value::Boolean(b) => b,
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "manifestYamlStream: quote_keys must be bool"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let result = self.manifest_yaml_stream(
                                    value,
                                    indent_array_in_object,
                                    c_document_end,
                                    quote_keys,
                                    span,
                                    source_id,
                                )?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle parseYaml
                            if id == chunk::NativeFuncId::ParseYaml {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let s = match args[0] {
                                    Value::String(idx) => {
                                        self.memory_manager.load_string(idx).to_string()
                                    }
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "parseYaml: argument must be a string"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let yaml_val: serde_yaml::Value = serde_yaml::from_str(&s)
                                    .map_err(|e| RuntimeError {
                                        span: span.clone(),
                                        message: format!("parseYaml: {}", e),
                                        source_id: source_id.clone(),
                                    })?;
                                let result =
                                    self.serde_yaml_to_jsonnet_value(yaml_val, span, source_id)?;
                                self.push(result)?;
                                continue;
                            }

                            // Handle manifestXmlJsonml
                            if id == chunk::NativeFuncId::ManifestXmlJsonml {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let result = self.manifest_xml_jsonml(value, span, source_id)?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle manifestTomlEx
                            if id == chunk::NativeFuncId::ManifestTomlEx {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let value = args[0];
                                let indent = match args[1] {
                                    Value::String(idx) => {
                                        self.memory_manager.load_string(idx).to_string()
                                    }
                                    _ => {
                                        return Err(RuntimeError {
                                            span,
                                            message: "manifestTomlEx: indent must be a string"
                                                .to_string(),
                                            source_id,
                                        });
                                    }
                                };
                                let result =
                                    self.manifest_toml_ex(value, &indent.clone(), span, source_id)?;
                                let idx = self.memory_manager.allocate_string(&result);
                                self.push(Value::String(idx.index))?;
                                continue;
                            }

                            // Handle std.minArray / std.maxArray with optional keyF and onEmpty
                            if matches!(
                                id,
                                chunk::NativeFuncId::MinArray | chunk::NativeFuncId::MaxArray
                            ) && args.len() >= 2
                            {
                                let arr_val = args[0];
                                let key_f = args.get(1).copied();
                                let on_empty = args.get(2).copied();

                                let arr_idx = match arr_val {
                                    Value::Array(a) => a,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.{} expected array, got {}",
                                                id.name(),
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };

                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();

                                if elements.is_empty() {
                                    match on_empty {
                                        Some(v) => {
                                            self.push(v)?;
                                            continue;
                                        }
                                        None => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!("std.{}: empty array", id.name()),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    }
                                }

                                let effective_key_f = match key_f {
                                    None | Some(Value::Null) => None,
                                    Some(v) => Some(v),
                                };

                                if let Some(key_f_val) = effective_key_f {
                                    let mut best_elem = elements[0];
                                    let mut best_key = {
                                        let mut roots = Vec::from(self.stack.clone());
                                        roots.extend_from_slice(&elements);
                                        roots.push(key_f_val);
                                        let mut open_upvalue_roots = Vec::new();
                                        let mut upvalue = self.open_upvalues;
                                        while let Some(uv_idx) = upvalue {
                                            open_upvalue_roots.push(uv_idx);
                                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                        }
                                        self.memory_manager
                                            .push_external_roots(roots, open_upvalue_roots);
                                        let k =
                                            self.call_value_with_one_arg(key_f_val, elements[0]);
                                        self.memory_manager.pop_external_roots();
                                        k?
                                    };

                                    for &elem in elements.iter().skip(1) {
                                        let key = {
                                            let mut roots = Vec::from(self.stack.clone());
                                            roots.extend_from_slice(&elements);
                                            roots.push(key_f_val);
                                            roots.push(best_elem);
                                            roots.push(best_key);
                                            let mut open_upvalue_roots = Vec::new();
                                            let mut upvalue = self.open_upvalues;
                                            while let Some(uv_idx) = upvalue {
                                                open_upvalue_roots.push(uv_idx);
                                                upvalue =
                                                    self.memory_manager.load_upvalue(uv_idx).next;
                                            }
                                            self.memory_manager
                                                .push_external_roots(roots, open_upvalue_roots);
                                            let k = self.call_value_with_one_arg(key_f_val, elem);
                                            self.memory_manager.pop_external_roots();
                                            k?
                                        };
                                        let ord = native::compare_values(
                                            key,
                                            best_key,
                                            &self.memory_manager,
                                        );
                                        let take = if id == chunk::NativeFuncId::MinArray {
                                            ord == std::cmp::Ordering::Less
                                        } else {
                                            ord == std::cmp::Ordering::Greater
                                        };
                                        if take {
                                            best_key = key;
                                            best_elem = elem;
                                        }
                                    }
                                    self.push(best_elem)?;
                                    continue;
                                } else {
                                    let mut best = elements[0];
                                    for &elem in elements.iter().skip(1) {
                                        let ord = native::compare_values(
                                            elem,
                                            best,
                                            &self.memory_manager,
                                        );
                                        let take = if id == chunk::NativeFuncId::MinArray {
                                            ord == std::cmp::Ordering::Less
                                        } else {
                                            ord == std::cmp::Ordering::Greater
                                        };
                                        if take {
                                            best = elem;
                                        }
                                    }
                                    self.push(best)?;
                                    continue;
                                }
                            }

                            // Handle std.extVar
                            if id == chunk::NativeFuncId::ExtVar {
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let key = match args[0] {
                                    Value::String(idx) => {
                                        self.memory_manager.load_string(idx).to_string()
                                    }
                                    other => {
                                        return Err(RuntimeError {
                                            span,
                                            message: format!(
                                                "std.extVar: argument must be a string, got {}",
                                                other.type_name()
                                            ),
                                            source_id,
                                        });
                                    }
                                };
                                match self.ext_vars.get(&key).copied() {
                                    Some(val) => {
                                        self.push(val)?;
                                        continue;
                                    }
                                    None => {
                                        return Err(RuntimeError {
                                            span,
                                            message: format!(
                                                "Undefined external variable: '{}'",
                                                key
                                            ),
                                            source_id,
                                        });
                                    }
                                }
                            }

                            // Handle set operations with keyF (3-arg forms)
                            if id == chunk::NativeFuncId::SetInter && args.len() == 3 {
                                let a_val = args[0];
                                let b_val = args[1];
                                let key_f = args[2];
                                let a_idx = match a_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setInter: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let b_idx = match b_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setInter: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let a_elems: Vec<Value> =
                                    self.memory_manager.load_array(a_idx).elements.clone();
                                let b_elems: Vec<Value> =
                                    self.memory_manager.load_array(b_idx).elements.clone();
                                let mut a_keys: Vec<Value> = Vec::with_capacity(a_elems.len());
                                for &elem in &a_elems {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&a_elems);
                                    roots.extend_from_slice(&b_elems);
                                    roots.extend_from_slice(&a_keys);
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
                                    a_keys.push(key?);
                                }
                                let mut b_keys: Vec<Value> = Vec::with_capacity(b_elems.len());
                                for &elem in &b_elems {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&a_elems);
                                    roots.extend_from_slice(&b_elems);
                                    roots.extend_from_slice(&a_keys);
                                    roots.extend_from_slice(&b_keys);
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
                                    b_keys.push(key?);
                                }
                                let mut result = Vec::new();
                                let (mut i, mut j) = (0usize, 0usize);
                                while i < a_elems.len() && j < b_elems.len() {
                                    let cmp = native::compare_values(
                                        a_keys[i],
                                        b_keys[j],
                                        &self.memory_manager,
                                    );
                                    match cmp {
                                        std::cmp::Ordering::Less => i += 1,
                                        std::cmp::Ordering::Greater => j += 1,
                                        std::cmp::Ordering::Equal => {
                                            result.push(a_elems[i]);
                                            i += 1;
                                            j += 1;
                                        }
                                    }
                                }
                                let alloc = self.memory_manager.allocate_array(result);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            if id == chunk::NativeFuncId::SetDiff && args.len() == 3 {
                                let a_val = args[0];
                                let b_val = args[1];
                                let key_f = args[2];
                                let a_idx = match a_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setDiff: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let b_idx = match b_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setDiff: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let a_elems: Vec<Value> =
                                    self.memory_manager.load_array(a_idx).elements.clone();
                                let b_elems: Vec<Value> =
                                    self.memory_manager.load_array(b_idx).elements.clone();
                                let mut a_keys: Vec<Value> = Vec::with_capacity(a_elems.len());
                                for &elem in &a_elems {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&a_elems);
                                    roots.extend_from_slice(&b_elems);
                                    roots.extend_from_slice(&a_keys);
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
                                    a_keys.push(key?);
                                }
                                let mut b_keys: Vec<Value> = Vec::with_capacity(b_elems.len());
                                for &elem in &b_elems {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&a_elems);
                                    roots.extend_from_slice(&b_elems);
                                    roots.extend_from_slice(&a_keys);
                                    roots.extend_from_slice(&b_keys);
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
                                    b_keys.push(key?);
                                }
                                let mut result = Vec::new();
                                let (mut i, mut j) = (0usize, 0usize);
                                while i < a_elems.len() {
                                    if j >= b_elems.len() {
                                        result.push(a_elems[i]);
                                        i += 1;
                                        continue;
                                    }
                                    let cmp = native::compare_values(
                                        a_keys[i],
                                        b_keys[j],
                                        &self.memory_manager,
                                    );
                                    match cmp {
                                        std::cmp::Ordering::Less => {
                                            result.push(a_elems[i]);
                                            i += 1;
                                        }
                                        std::cmp::Ordering::Greater => {
                                            j += 1;
                                        }
                                        std::cmp::Ordering::Equal => {
                                            i += 1;
                                            j += 1;
                                        }
                                    }
                                }
                                let alloc = self.memory_manager.allocate_array(result);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            if id == chunk::NativeFuncId::SetMember && args.len() == 3 {
                                let x_val = args[0];
                                let arr_val = args[1];
                                let key_f = args[2];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setMember: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let arr_elems: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let x_key = {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&arr_elems);
                                    roots.push(key_f);
                                    roots.push(x_val);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let k = self.call_value_with_one_arg(key_f, x_val);
                                    self.memory_manager.pop_external_roots();
                                    k?
                                };
                                let mut lo = 0usize;
                                let mut hi = arr_elems.len();
                                let mut found = false;
                                while lo < hi {
                                    let mid = lo + (hi - lo) / 2;
                                    let mid_key = {
                                        let mut roots = Vec::from(self.stack.clone());
                                        roots.extend_from_slice(&arr_elems);
                                        roots.push(key_f);
                                        roots.push(x_val);
                                        roots.push(x_key);
                                        let mut open_upvalue_roots = Vec::new();
                                        let mut upvalue = self.open_upvalues;
                                        while let Some(uv_idx) = upvalue {
                                            open_upvalue_roots.push(uv_idx);
                                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                        }
                                        self.memory_manager
                                            .push_external_roots(roots, open_upvalue_roots);
                                        let k = self.call_value_with_one_arg(key_f, arr_elems[mid]);
                                        self.memory_manager.pop_external_roots();
                                        k?
                                    };
                                    let cmp = native::compare_values(
                                        x_key,
                                        mid_key,
                                        &self.memory_manager,
                                    );
                                    match cmp {
                                        std::cmp::Ordering::Equal => {
                                            found = true;
                                            break;
                                        }
                                        std::cmp::Ordering::Less => hi = mid,
                                        std::cmp::Ordering::Greater => lo = mid + 1,
                                    }
                                }
                                self.push(Value::Boolean(found))?;
                                continue;
                            }

                            if id == chunk::NativeFuncId::SetUnion && args.len() == 3 {
                                let a_val = args[0];
                                let b_val = args[1];
                                let key_f = args[2];
                                let a_idx = match a_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setUnion: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let b_idx = match b_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "setUnion: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let mut combined =
                                    self.memory_manager.load_array(a_idx).elements.clone();
                                combined.extend_from_slice(
                                    &self.memory_manager.load_array(b_idx).elements.clone(),
                                );
                                let mut keys: Vec<Value> = Vec::with_capacity(combined.len());
                                for &elem in &combined {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&combined);
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
                                let mut indexed: Vec<usize> = (0..combined.len()).collect();
                                {
                                    let mm = &self.memory_manager;
                                    indexed.sort_by(|&a, &b| {
                                        native::compare_values(keys[a], keys[b], mm)
                                    });
                                }
                                let sorted_elems: Vec<Value> =
                                    indexed.iter().map(|&i| combined[i]).collect();
                                let sorted_keys: Vec<Value> =
                                    indexed.iter().map(|&i| keys[i]).collect();
                                let mut result: Vec<Value> = Vec::new();
                                let mut last_key: Option<Value> = None;
                                for (elem, key) in sorted_elems.iter().zip(sorted_keys.iter()) {
                                    if let Some(lk) = last_key {
                                        if native::compare_values(lk, *key, &self.memory_manager)
                                            == std::cmp::Ordering::Equal
                                        {
                                            continue;
                                        }
                                    }
                                    result.push(*elem);
                                    last_key = Some(*key);
                                }
                                let alloc = self.memory_manager.allocate_array(result);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            // Handle std.groupBy
                            if id == chunk::NativeFuncId::GroupBy {
                                let arr_val = args[0];
                                let key_f = args[1];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.groupBy: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let mut group_order: Vec<String> = Vec::new();
                                let mut groups: std::collections::HashMap<String, Vec<Value>> =
                                    std::collections::HashMap::new();
                                for &elem in &elements {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f);
                                    for group_elems in groups.values() {
                                        roots.extend_from_slice(group_elems);
                                    }
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let key_val = self.call_value_with_one_arg(key_f, elem);
                                    self.memory_manager.pop_external_roots();
                                    let key_val = key_val?;
                                    let key_str = match key_val {
                                        Value::String(idx) => {
                                            self.memory_manager.load_string(idx).to_string()
                                        }
                                        other => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!(
                                                    "std.groupBy: keyF must return string, got {}",
                                                    other.type_name()
                                                ),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    };
                                    if !groups.contains_key(&key_str) {
                                        group_order.push(key_str.clone());
                                        groups.insert(key_str.clone(), Vec::new());
                                    }
                                    groups.get_mut(&key_str).unwrap().push(elem);
                                }
                                let mut properties: std::collections::HashMap<
                                    chunk::StringIndex,
                                    memory_manager::ObjectField,
                                > = std::collections::HashMap::new();
                                for key_str in &group_order {
                                    let group_elems = groups.remove(key_str).unwrap_or_default();
                                    let arr_alloc = self.memory_manager.allocate_array(group_elems);
                                    let k_alloc = self.memory_manager.allocate_string(key_str);
                                    properties.insert(
                                        k_alloc.index,
                                        memory_manager::ObjectField {
                                            value: Value::Array(arr_alloc.index),
                                            super_obj: None,
                                            visibility: FieldVisibility::Visible,
                                        },
                                    );
                                }
                                let obj_alloc = self
                                    .memory_manager
                                    .allocate_object_with_properties(properties);
                                self.push(Value::Object(obj_alloc.index))?;
                                continue;
                            }

                            // Handle std.sortBy
                            if id == chunk::NativeFuncId::SortBy {
                                let arr_val = args[0];
                                let key_f = args[1];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.sortBy: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let mut keyed: Vec<(Value, Value)> =
                                    Vec::with_capacity(elements.len());
                                for &elem in &elements {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f);
                                    for (k, v) in &keyed {
                                        roots.push(*k);
                                        roots.push(*v);
                                    }
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
                                    keyed.push((key?, elem));
                                }
                                keyed.sort_by(|(ka, _), (kb, _)| {
                                    native::compare_values(*ka, *kb, &self.memory_manager)
                                });
                                let sorted: Vec<Value> =
                                    keyed.into_iter().map(|(_, v)| v).collect();
                                let alloc = self.memory_manager.allocate_array(sorted);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            // Handle std.countBy
                            if id == chunk::NativeFuncId::CountBy {
                                let arr_val = args[0];
                                let key_f = args[1];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.countBy: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let mut group_order: Vec<String> = Vec::new();
                                let mut counts: std::collections::HashMap<String, u64> =
                                    std::collections::HashMap::new();
                                for &elem in &elements {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let key_val = self.call_value_with_one_arg(key_f, elem);
                                    self.memory_manager.pop_external_roots();
                                    let key_val = key_val?;
                                    let key_str = match key_val {
                                        Value::String(idx) => {
                                            self.memory_manager.load_string(idx).to_string()
                                        }
                                        other => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!(
                                                    "std.countBy: keyF must return string, got {}",
                                                    other.type_name()
                                                ),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    };
                                    if !counts.contains_key(&key_str) {
                                        group_order.push(key_str.clone());
                                    }
                                    *counts.entry(key_str).or_insert(0) += 1;
                                }
                                let mut properties: std::collections::HashMap<
                                    chunk::StringIndex,
                                    memory_manager::ObjectField,
                                > = std::collections::HashMap::new();
                                for key_str in &group_order {
                                    let count = counts[key_str];
                                    let k_alloc = self.memory_manager.allocate_string(key_str);
                                    properties.insert(
                                        k_alloc.index,
                                        memory_manager::ObjectField {
                                            value: Value::Number(count as f64),
                                            super_obj: None,
                                            visibility: FieldVisibility::Visible,
                                        },
                                    );
                                }
                                let obj_alloc = self
                                    .memory_manager
                                    .allocate_object_with_properties(properties);
                                self.push(Value::Object(obj_alloc.index))?;
                                continue;
                            }

                            // Handle std.uniqBy
                            if id == chunk::NativeFuncId::UniqBy {
                                let arr_val = args[0];
                                let key_f = args[1];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.uniqBy: expected array, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                let mut seen: std::collections::HashSet<String> =
                                    std::collections::HashSet::new();
                                let mut result: Vec<Value> = Vec::new();
                                for &elem in &elements {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.extend_from_slice(&result);
                                    roots.push(key_f);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let key_val = self.call_value_with_one_arg(key_f, elem);
                                    self.memory_manager.pop_external_roots();
                                    let key_val = key_val?;
                                    let key_str = match key_val {
                                        Value::String(idx) => {
                                            self.memory_manager.load_string(idx).to_string()
                                        }
                                        other => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!(
                                                    "std.uniqBy: keyF must return string, got {}",
                                                    other.type_name()
                                                ),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    };
                                    if seen.insert(key_str) {
                                        result.push(elem);
                                    }
                                }
                                let alloc = self.memory_manager.allocate_array(result);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            // Handle std.minBy and std.maxBy
                            if id == chunk::NativeFuncId::MinBy || id == chunk::NativeFuncId::MaxBy
                            {
                                let arr_val = args[0];
                                let key_f = args[1];
                                let arr_idx = match arr_val {
                                    Value::Array(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.{}: expected array, got {}",
                                                id.name(),
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let elements: Vec<Value> =
                                    self.memory_manager.load_array(arr_idx).elements.clone();
                                if elements.is_empty() {
                                    return Err(RuntimeError {
                                        span: self.get_current_span(),
                                        message: format!(
                                            "std.{}: array must not be empty",
                                            id.name()
                                        ),
                                        source_id: self.current_chunk().source_id.to_string(),
                                    });
                                }
                                let (mut best_key, mut best_elem) = {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f);
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let k = self.call_value_with_one_arg(key_f, elements[0]);
                                    self.memory_manager.pop_external_roots();
                                    (k?, elements[0])
                                };
                                for &elem in elements.iter().skip(1) {
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.extend_from_slice(&elements);
                                    roots.push(key_f);
                                    roots.push(best_key);
                                    roots.push(best_elem);
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
                                    let cmp =
                                        native::compare_values(key, best_key, &self.memory_manager);
                                    let take = if id == chunk::NativeFuncId::MinBy {
                                        cmp == std::cmp::Ordering::Less
                                    } else {
                                        cmp == std::cmp::Ordering::Greater
                                    };
                                    if take {
                                        best_key = key;
                                        best_elem = elem;
                                    }
                                }
                                self.push(best_elem)?;
                                continue;
                            }

                            // Handle std.toPairs
                            if id == chunk::NativeFuncId::ToPairs {
                                let obj_val = args[0];
                                let o_idx = match obj_val {
                                    Value::Object(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.toPairs: expected object, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let field_data: Vec<(chunk::StringIndex, Value)> = {
                                    let obj = self.memory_manager.load_object(o_idx);
                                    obj.properties
                                        .iter()
                                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                        .map(|(k, f)| (*k, f.value))
                                        .collect()
                                };
                                let mut pairs: Vec<Value> = Vec::with_capacity(field_data.len());
                                for (k_idx, raw_val) in &field_data {
                                    let k_idx = *k_idx;
                                    let raw_val = *raw_val;
                                    let evaled_val = match raw_val {
                                        Value::Closure(closure_idx) => {
                                            self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                        }
                                        other => other,
                                    };
                                    let key_str =
                                        self.memory_manager.load_string(k_idx).to_string();
                                    let k_alloc = self.memory_manager.allocate_string(&key_str);
                                    let k_val = Value::String(k_alloc.index);
                                    let pair_alloc =
                                        self.memory_manager.allocate_array(vec![k_val, evaled_val]);
                                    pairs.push(Value::Array(pair_alloc.index));
                                }
                                let alloc = self.memory_manager.allocate_array(pairs);
                                self.push(Value::Array(alloc.index))?;
                                continue;
                            }

                            // Handle std.mapKeys
                            if id == chunk::NativeFuncId::MapKeys {
                                let func_val = args[0];
                                let obj_val = args[1];
                                let o_idx = match obj_val {
                                    Value::Object(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.mapKeys: expected object, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = {
                                    let obj = self.memory_manager.load_object(o_idx);
                                    obj.properties
                                        .iter()
                                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                        .map(|(k, f)| (*k, f.value, f.visibility))
                                        .collect()
                                };
                                let mut new_properties: std::collections::HashMap<
                                    chunk::StringIndex,
                                    memory_manager::ObjectField,
                                > = std::collections::HashMap::new();
                                for (k_idx, raw_val, vis) in &field_data {
                                    let k_idx = *k_idx;
                                    let raw_val = *raw_val;
                                    let vis = *vis;
                                    let evaled_val = match raw_val {
                                        Value::Closure(closure_idx) => {
                                            self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                        }
                                        other => other,
                                    };
                                    let key_str =
                                        self.memory_manager.load_string(k_idx).to_string();
                                    let key_alloc = self.memory_manager.allocate_string(&key_str);
                                    let key_val = Value::String(key_alloc.index);
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.push(func_val);
                                    roots.push(evaled_val);
                                    roots.push(key_val);
                                    for f in new_properties.values() {
                                        roots.push(f.value);
                                    }
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let new_key_val =
                                        self.call_value_with_one_arg(func_val, key_val);
                                    self.memory_manager.pop_external_roots();
                                    let new_key_val = new_key_val?;
                                    let new_key_str = match new_key_val {
                                        Value::String(idx) => {
                                            self.memory_manager.load_string(idx).to_string()
                                        }
                                        other => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!(
                                                    "std.mapKeys: func must return string, got {}",
                                                    other.type_name()
                                                ),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    };
                                    let new_k_alloc =
                                        self.memory_manager.allocate_string(&new_key_str);
                                    new_properties.insert(
                                        new_k_alloc.index,
                                        memory_manager::ObjectField {
                                            value: evaled_val,
                                            super_obj: None,
                                            visibility: vis,
                                        },
                                    );
                                }
                                let alloc = self
                                    .memory_manager
                                    .allocate_object_with_properties(new_properties);
                                self.push(Value::Object(alloc.index))?;
                                continue;
                            }

                            // Handle std.filterObject
                            if id == chunk::NativeFuncId::FilterObject {
                                let func_val = args[0];
                                let obj_val = args[1];
                                let o_idx = match obj_val {
                                    Value::Object(i) => i,
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.filterObject: expected object, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = {
                                    let obj = self.memory_manager.load_object(o_idx);
                                    obj.properties
                                        .iter()
                                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                                        .map(|(k, f)| (*k, f.value, f.visibility))
                                        .collect()
                                };
                                let mut kept_properties: std::collections::HashMap<
                                    chunk::StringIndex,
                                    memory_manager::ObjectField,
                                > = std::collections::HashMap::new();
                                for (k_idx, raw_val, vis) in &field_data {
                                    let k_idx = *k_idx;
                                    let raw_val = *raw_val;
                                    let vis = *vis;
                                    let evaled_val = match raw_val {
                                        Value::Closure(closure_idx) => {
                                            self.execute_thunk_sync(closure_idx, Some(o_idx), None)?
                                        }
                                        other => other,
                                    };
                                    let key_str =
                                        self.memory_manager.load_string(k_idx).to_string();
                                    let key_alloc = self.memory_manager.allocate_string(&key_str);
                                    let key_val = Value::String(key_alloc.index);
                                    let mut roots = Vec::from(self.stack.clone());
                                    roots.push(func_val);
                                    roots.push(evaled_val);
                                    roots.push(key_val);
                                    for f in kept_properties.values() {
                                        roots.push(f.value);
                                    }
                                    let mut open_upvalue_roots = Vec::new();
                                    let mut upvalue = self.open_upvalues;
                                    while let Some(uv_idx) = upvalue {
                                        open_upvalue_roots.push(uv_idx);
                                        upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                    }
                                    self.memory_manager
                                        .push_external_roots(roots, open_upvalue_roots);
                                    let keep = self
                                        .call_value_with_two_args(func_val, key_val, evaled_val);
                                    self.memory_manager.pop_external_roots();
                                    match keep? {
                                        Value::Boolean(true) => {
                                            kept_properties.insert(
                                                k_idx,
                                                memory_manager::ObjectField {
                                                    value: evaled_val,
                                                    super_obj: None,
                                                    visibility: vis,
                                                },
                                            );
                                        }
                                        Value::Boolean(false) => {}
                                        other => {
                                            return Err(RuntimeError {
                                                span: self.get_current_span(),
                                                message: format!(
                                                    "std.filterObject: func must return bool, got {}",
                                                    other.type_name()
                                                ),
                                                source_id: self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string(),
                                            });
                                        }
                                    }
                                }
                                let alloc = self
                                    .memory_manager
                                    .allocate_object_with_properties(kept_properties);
                                self.push(Value::Object(alloc.index))?;
                                continue;
                            }

                            // Handle std.objectFlatten
                            if id == chunk::NativeFuncId::ObjectFlatten {
                                let obj_val = args[0];
                                let sep_val = args[1];
                                let sep = match sep_val {
                                    Value::String(idx) => {
                                        self.memory_manager.load_string(idx).to_string()
                                    }
                                    other => {
                                        return Err(RuntimeError {
                                            span: self.get_current_span(),
                                            message: format!(
                                                "std.objectFlatten: sep must be string, got {}",
                                                other.type_name()
                                            ),
                                            source_id: self.current_chunk().source_id.to_string(),
                                        });
                                    }
                                };
                                let mut flat_fields: Vec<(String, Value)> = Vec::new();
                                self.flatten_object_recursive(
                                    obj_val,
                                    &sep,
                                    String::new(),
                                    &mut flat_fields,
                                )?;
                                let mut properties: std::collections::HashMap<
                                    chunk::StringIndex,
                                    memory_manager::ObjectField,
                                > = std::collections::HashMap::new();
                                for (k_str, v) in flat_fields {
                                    let k_alloc = self.memory_manager.allocate_string(&k_str);
                                    properties.insert(
                                        k_alloc.index,
                                        memory_manager::ObjectField {
                                            value: v,
                                            super_obj: None,
                                            visibility: FieldVisibility::Visible,
                                        },
                                    );
                                }
                                let obj_alloc = self
                                    .memory_manager
                                    .allocate_object_with_properties(properties);
                                self.push(Value::Object(obj_alloc.index))?;
                                continue;
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

    /// Check if two values are equal according to Jsonnet semantics.
    /// Note: this only performs a structural comparison, it does not
    /// compare object identity for anything except functions and closures.
    pub fn values_equal(&mut self, a: &Value, b: &Value) -> Result<bool, RuntimeError> {
        // Protect values from GC during comparison
        self.memory_manager
            .external_roots
            .push(vec![a.clone(), b.clone()]);

        let result = (|| match (a, b) {
            (Value::Null, Value::Null) => Ok(true),
            (Value::Boolean(a), Value::Boolean(b)) => Ok(a == b),
            (Value::Number(a), Value::Number(b)) => Ok(a == b),
            (Value::String(a), Value::String(b)) => Ok(a == b),
            (Value::Array(a_idx), Value::Array(b_idx)) => {
                if a_idx == b_idx {
                    return Ok(true);
                }
                let a_elements = self.memory_manager.load_array(*a_idx).elements.clone();
                let b_elements = self.memory_manager.load_array(*b_idx).elements.clone();

                self.memory_manager.external_roots.push(a_elements.clone());
                self.memory_manager.external_roots.push(b_elements.clone());

                let res = (|| {
                    if a_elements.len() != b_elements.len() {
                        return Ok(false);
                    }
                    for (v_a, v_b) in a_elements.iter().zip(b_elements.iter()) {
                        if !self.values_equal(v_a, v_b)? {
                            return Ok(false);
                        }
                    }
                    Ok(true)
                })();

                self.memory_manager.external_roots.pop();
                self.memory_manager.external_roots.pop();
                res
            }
            (Value::Object(a_idx), Value::Object(b_idx)) => {
                if a_idx == b_idx {
                    return Ok(true);
                }

                let a_fields = self.get_visible_fields(*a_idx)?;
                let a_vals: Vec<Value> = a_fields.values().cloned().collect();
                self.memory_manager.external_roots.push(a_vals);

                let b_fields_result = self.get_visible_fields(*b_idx);
                if b_fields_result.is_err() {
                    self.memory_manager.external_roots.pop();
                    return b_fields_result.map(|_| unreachable!());
                }
                let b_fields = b_fields_result.unwrap();

                let b_vals: Vec<Value> = b_fields.values().cloned().collect();
                self.memory_manager.external_roots.push(b_vals);

                let res = (|| {
                    if a_fields.len() != b_fields.len() {
                        return Ok(false);
                    }

                    for (name, val_a) in a_fields {
                        match b_fields.get(&name) {
                            Some(val_b) => {
                                if !self.values_equal(&val_a, val_b)? {
                                    return Ok(false);
                                }
                            }
                            None => return Ok(false),
                        }
                    }
                    Ok(true)
                })();

                self.memory_manager.external_roots.pop();
                self.memory_manager.external_roots.pop();
                res
            }
            (Value::Function(a), Value::Function(b)) => Ok(a == b),
            (Value::Closure(a), Value::Closure(b)) => Ok(a == b),
            (Value::Binary(a), Value::Binary(b)) => Ok(a == b),
            _ => Ok(false),
        })();

        self.memory_manager.external_roots.pop();
        result
    }

    /// Helper to get all visible fields of an object with their evaluated values.
    fn get_visible_fields(
        &mut self,
        obj_idx: ObjectIndex,
    ) -> Result<std::collections::HashMap<StringIndex, Value>, RuntimeError> {
        let obj = self.memory_manager.load_object(obj_idx);
        let mut visible_fields = std::collections::HashMap::new();

        let properties: Vec<(StringIndex, ObjectField)> = obj
            .properties
            .iter()
            .map(|(&k, f)| (k, f.clone()))
            .collect();

        for (name, field) in properties {
            if field.visibility != FieldVisibility::Hidden {
                let current_vals: Vec<Value> = visible_fields.values().cloned().collect();
                self.memory_manager.external_roots.push(current_vals);

                let val_res = match field.value {
                    Value::Closure(closure_idx) => {
                        self.execute_thunk_sync(closure_idx, Some(obj_idx), None)
                    }
                    other => Ok(other),
                };

                self.memory_manager.external_roots.pop();

                let val = val_res?;
                visible_fields.insert(name, val);
            }
        }
        Ok(visible_fields)
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

        // Root all ext_vars values so GC doesn't collect them
        for val in self.ext_vars.values() {
            roots.push(*val);
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

    /// Convert a serde_json::Value to a Jsonnet Value
    fn json_to_jsonnet_value(&mut self, val: &serde_json::Value) -> Result<Value, RuntimeError> {
        match val {
            serde_json::Value::Null => Ok(Value::Null),
            serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
            serde_json::Value::Number(n) => Ok(Value::Number(n.as_f64().unwrap_or(f64::NAN))),
            serde_json::Value::String(s) => {
                let alloc = self.memory_manager.allocate_string(s);
                Ok(Value::String(alloc.index))
            }
            serde_json::Value::Array(arr) => {
                let mut elements = Vec::with_capacity(arr.len());
                for item in arr {
                    elements.push(self.json_to_jsonnet_value(item)?);
                }
                let alloc = self.memory_manager.allocate_array(elements);
                Ok(Value::Array(alloc.index))
            }
            serde_json::Value::Object(map) => {
                let mut props = std::collections::HashMap::new();
                for (k, v) in map {
                    let key_idx = self.memory_manager.allocate_string(k).index;
                    let val = self.json_to_jsonnet_value(v)?;
                    props.insert(
                        key_idx,
                        memory_manager::ObjectField {
                            value: val,
                            super_obj: None,
                            visibility: chunk::FieldVisibility::Visible,
                        },
                    );
                }
                let alloc = self.memory_manager.allocate_object_with_properties(props);
                Ok(Value::Object(alloc.index))
            }
        }
    }

    /// Serialize a Value to a JSON string with configurable formatting
    fn manifest_json_value(
        &mut self,
        value: Value,
        indent: &str,
        newline: &str,
        key_val_sep: &str,
        depth: usize,
        span: Range<usize>,
        source_id: &str,
    ) -> Result<String, RuntimeError> {
        // Force closures/imports
        let value = self.force_value(value)?;
        let value = match value {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };

        match value {
            Value::Null => Ok("null".to_string()),
            Value::Boolean(true) => Ok("true".to_string()),
            Value::Boolean(false) => Ok("false".to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestJson: cannot serialize NaN".to_string(),
                        source_id: source_id.to_string(),
                    });
                }
                if n.is_infinite() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestJson: cannot serialize Infinite".to_string(),
                        source_id: source_id.to_string(),
                    });
                }
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::String(s_idx) => {
                let s = self.memory_manager.load_string(s_idx).to_string();
                let mut out = String::from("\"");
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                Ok(out)
            }
            Value::Array(a_idx) => {
                let elements = self.memory_manager.load_array(a_idx).elements.clone();
                if elements.is_empty() {
                    return Ok("[ ]".to_string());
                }
                let item_indent = indent.repeat(depth + 1);
                let close_indent = indent.repeat(depth);
                let mut items = Vec::with_capacity(elements.len());
                for elem in elements {
                    let s = self.manifest_json_value(
                        elem,
                        indent,
                        newline,
                        key_val_sep,
                        depth + 1,
                        span.clone(),
                        source_id,
                    )?;
                    items.push(format!("{}{}", item_indent, s));
                }
                Ok(format!(
                    "[{}{}{}{}]",
                    newline,
                    items.join(&format!(",{}", newline)),
                    newline,
                    close_indent,
                ))
            }
            Value::Object(o_idx) => {
                // Collect visible fields
                let field_data: Vec<(StringIndex, Value, Option<ObjectIndex>)> = {
                    let obj = self.memory_manager.load_object(o_idx);
                    let mut fields: Vec<(StringIndex, Value, Option<ObjectIndex>)> = obj
                        .properties
                        .iter()
                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                        .map(|(k, f)| (*k, f.value, f.super_obj))
                        .collect();
                    // Sort by field name
                    fields.sort_by(|(ka, _, _), (kb, _, _)| {
                        let sa = obj.properties.get(ka).map(|_| {
                            // We need to sort by key string; collect key strings separately
                            *ka
                        });
                        let _ = sa;
                        ka.cmp(kb)
                    });
                    fields
                };

                // Sort fields by string name
                let mut sorted_fields: Vec<(String, Value, Option<ObjectIndex>)> = field_data
                    .into_iter()
                    .map(|(k, v, so)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                sorted_fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

                if sorted_fields.is_empty() {
                    return Ok("{ }".to_string());
                }

                let item_indent = indent.repeat(depth + 1);
                let close_indent = indent.repeat(depth);
                let mut pairs = Vec::with_capacity(sorted_fields.len());
                for (key_str, field_val, super_obj) in sorted_fields {
                    // Force field value (thunk)
                    let forced_val = match field_val {
                        Value::Closure(c) => self.execute_thunk_sync(c, Some(o_idx), super_obj)?,
                        other => other,
                    };
                    let val_s = self.manifest_json_value(
                        forced_val,
                        indent,
                        newline,
                        key_val_sep,
                        depth + 1,
                        span.clone(),
                        source_id,
                    )?;
                    // JSON-escape the key
                    let mut key_out = String::from("\"");
                    for ch in key_str.chars() {
                        match ch {
                            '"' => key_out.push_str("\\\""),
                            '\\' => key_out.push_str("\\\\"),
                            '\n' => key_out.push_str("\\n"),
                            '\r' => key_out.push_str("\\r"),
                            '\t' => key_out.push_str("\\t"),
                            c if (c as u32) < 0x20 => {
                                key_out.push_str(&format!("\\u{:04x}", c as u32));
                            }
                            c => key_out.push(c),
                        }
                    }
                    key_out.push('"');
                    pairs.push(format!(
                        "{}{}{}{}",
                        item_indent, key_out, key_val_sep, val_s
                    ));
                }
                Ok(format!(
                    "{{{}{}{}{}}}",
                    newline,
                    pairs.join(&format!(",{}", newline)),
                    newline,
                    close_indent,
                ))
            }
            _ => Err(RuntimeError {
                span,
                message: "std.manifestJson: cannot manifest function".to_string(),
                source_id: source_id.to_string(),
            }),
        }
    }

    fn flatten_object_recursive(
        &mut self,
        value: Value,
        sep: &str,
        prefix: String,
        out: &mut Vec<(String, Value)>,
    ) -> Result<(), RuntimeError> {
        match value {
            Value::Object(o_idx) => {
                let fields: Vec<(chunk::StringIndex, Value)> = {
                    let obj = self.memory_manager.load_object(o_idx);
                    obj.properties
                        .iter()
                        .filter(|(_, f)| f.visibility != chunk::FieldVisibility::Hidden)
                        .map(|(k, f)| (*k, f.value))
                        .collect()
                };
                for (k_idx, raw_val) in &fields {
                    let k_idx = *k_idx;
                    let raw_val = *raw_val;
                    let k_str = self.memory_manager.load_string(k_idx).to_string();
                    let full_key = if prefix.is_empty() {
                        k_str.clone()
                    } else {
                        format!("{}{}{}", prefix, sep, k_str)
                    };
                    let forced_v = match raw_val {
                        Value::Closure(closure_idx) => {
                            // Protect accumulated out values and remaining fields from GC
                            let mut temp_vals: Vec<Value> = out.iter().map(|(_, v)| *v).collect();
                            for (_, fv) in fields.iter() {
                                temp_vals.push(*fv);
                            }
                            self.memory_manager.external_roots.push(temp_vals);
                            let result = self.execute_thunk_sync(closure_idx, Some(o_idx), None)?;
                            self.memory_manager.external_roots.pop();
                            result
                        }
                        other => other,
                    };
                    self.flatten_object_recursive(forced_v, sep, full_key, out)?;
                }
            }
            other => {
                out.push((prefix, other));
            }
        }
        Ok(())
    }

    fn merge_patch_value(&mut self, target: Value, patch: Value) -> Result<Value, RuntimeError> {
        let patch_idx = match patch {
            Value::Object(o) => o,
            _ => return Ok(patch),
        };
        let patch_props: Vec<(StringIndex, memory_manager::ObjectField)> = {
            let obj = self.memory_manager.load_object(patch_idx);
            obj.properties
                .iter()
                .map(|(k, f)| (*k, f.clone()))
                .collect()
        };
        let t_idx_opt = match target {
            Value::Object(t_idx) => Some(t_idx),
            _ => None,
        };
        let mut result: std::collections::HashMap<StringIndex, memory_manager::ObjectField> =
            match target {
                Value::Object(t_idx) => {
                    let obj = self.memory_manager.load_object(t_idx);
                    obj.properties
                        .iter()
                        .map(|(k, f)| (*k, f.clone()))
                        .collect()
                }
                _ => std::collections::HashMap::new(),
            };
        for (key, field) in patch_props {
            // Must evaluate field value to see if it's null
            let mut val = field.value;
            if let Value::Closure(closure_idx) = val {
                // Protect result HashMap during thunk execution
                let mut temp_vals = Vec::new();
                for f in result.values() {
                    temp_vals.push(f.value);
                }
                self.memory_manager.external_roots.push(temp_vals);

                val = self.execute_thunk_sync(closure_idx, Some(patch_idx), None)?;

                self.memory_manager.external_roots.pop();
            }
            if val == Value::Null {
                result.remove(&key);
            } else {
                let mut existing = result.get(&key).map(|f| f.value).unwrap_or(Value::Null);

                if let Value::Closure(c_idx) = existing {
                    let mut temp_vals = Vec::new();
                    for f in result.values() {
                        temp_vals.push(f.value);
                    }
                    temp_vals.push(val);
                    self.memory_manager.external_roots.push(temp_vals);

                    existing = self.execute_thunk_sync(c_idx, t_idx_opt, None)?;

                    self.memory_manager.external_roots.pop();
                }

                // Protect result during recursion
                let mut temp_vals = Vec::new();
                for f in result.values() {
                    temp_vals.push(f.value);
                }
                temp_vals.push(val);
                temp_vals.push(existing);
                self.memory_manager.external_roots.push(temp_vals);

                let merged = self.merge_patch_value(existing, val)?;

                self.memory_manager.external_roots.pop();

                result.insert(
                    key,
                    memory_manager::ObjectField {
                        value: merged,
                        super_obj: field.super_obj,
                        visibility: field.visibility,
                    },
                );
            }
        }
        let alloc = self.memory_manager.allocate_object_with_properties(result);
        Ok(Value::Object(alloc.index))
    }

    fn prune_value(&mut self, val: Value) -> Result<Value, RuntimeError> {
        match val {
            Value::Array(a_idx) => {
                let elements = self.memory_manager.load_array(a_idx).elements.clone();
                let mut pruned = Vec::new();
                for elem in elements {
                    // Protect pruned so far
                    self.memory_manager.external_roots.push(pruned.clone());
                    let pruned_elem = self.prune_value(elem)?;
                    self.memory_manager.external_roots.pop();

                    if !self.is_prunable(pruned_elem)? {
                        pruned.push(pruned_elem);
                    }
                }
                let alloc = self.memory_manager.allocate_array(pruned);
                Ok(Value::Array(alloc.index))
            }
            Value::Object(o_idx) => {
                let field_data: Vec<(
                    StringIndex,
                    Value,
                    Option<ObjectIndex>,
                    chunk::FieldVisibility,
                )> = {
                    let obj = self.memory_manager.load_object(o_idx);
                    obj.properties
                        .iter()
                        .filter(|(_, f)| f.visibility != chunk::FieldVisibility::Hidden)
                        .map(|(k, f)| (*k, f.value, f.super_obj, f.visibility))
                        .collect()
                };
                let mut new_props: std::collections::HashMap<
                    StringIndex,
                    memory_manager::ObjectField,
                > = std::collections::HashMap::new();
                for (k, v, super_obj, vis) in field_data {
                    let mut eval_v = v;
                    if let Value::Closure(closure_idx) = eval_v {
                        let mut temp_vals = Vec::new();
                        for f in new_props.values() {
                            temp_vals.push(f.value);
                        }
                        self.memory_manager.external_roots.push(temp_vals);
                        eval_v = self.execute_thunk_sync(closure_idx, Some(o_idx), None)?;
                        self.memory_manager.external_roots.pop();
                    }

                    let mut temp_vals = Vec::new();
                    for f in new_props.values() {
                        temp_vals.push(f.value);
                    }
                    self.memory_manager.external_roots.push(temp_vals);
                    let pruned_v = self.prune_value(eval_v)?;
                    self.memory_manager.external_roots.pop();

                    if !self.is_prunable(pruned_v)? {
                        new_props.insert(
                            k,
                            memory_manager::ObjectField {
                                value: pruned_v,
                                super_obj,
                                visibility: vis,
                            },
                        );
                    }
                }
                let alloc = self
                    .memory_manager
                    .allocate_object_with_properties(new_props);
                Ok(Value::Object(alloc.index))
            }
            other => Ok(other),
        }
    }

    fn is_prunable(&mut self, val: Value) -> Result<bool, RuntimeError> {
        match val {
            Value::Null => Ok(true),
            Value::Array(a_idx) => Ok(self.memory_manager.load_array(a_idx).elements.is_empty()),
            Value::Object(o_idx) => {
                let properties: Vec<(StringIndex, memory_manager::ObjectField)> = self
                    .memory_manager
                    .load_object(o_idx)
                    .properties
                    .iter()
                    .map(|(k, f)| (*k, f.clone()))
                    .collect();

                let mut all_prunable = true;
                for (_k, field) in properties {
                    if field.visibility != chunk::FieldVisibility::Hidden {
                        let mut v = field.value;
                        if let Value::Closure(closure_idx) = v {
                            self.memory_manager.external_roots.push(vec![val]);
                            v = self.execute_thunk_sync(closure_idx, Some(o_idx), None)?;
                            self.memory_manager.external_roots.pop();
                        }
                        if !self.is_prunable(v)? {
                            all_prunable = false;
                            break;
                        }
                    }
                }
                Ok(all_prunable)
            }
            _ => Ok(false),
        }
    }

    fn uniq_value(
        &mut self,
        arr_val: Value,
        key_func_opt: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        let arr_idx = match arr_val {
            Value::Array(a) => a,
            _ => {
                return Err(RuntimeError {
                    span: self.get_current_span(),
                    message: "std.uniq: first argument must be an array".to_string(),
                    source_id: self.current_chunk().source_id.to_string(),
                });
            }
        };
        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
        let mut result = Vec::new();
        let mut last_key: Option<Value> = None;

        for elem in elements {
            self.memory_manager.external_roots.push(result.clone());
            if let Some(k) = last_key {
                self.memory_manager.external_roots.push(vec![k, elem]);
            } else {
                self.memory_manager.external_roots.push(vec![elem]);
            }

            let key = match key_func_opt {
                Some(f) => self.call_value_with_one_arg(f, elem)?,
                None => elem,
            };

            let mut duplicate = false;
            if let Some(lk) = last_key {
                if self.values_equal(&lk, &key)? {
                    duplicate = true;
                }
            }

            self.memory_manager.external_roots.pop(); // pop keys
            self.memory_manager.external_roots.pop(); // pop result

            if !duplicate {
                result.push(elem);
                last_key = Some(key);
            }
        }
        let arr_alloc = self.memory_manager.allocate_array(result);
        Ok(Value::Array(arr_alloc.index))
    }

    /// Manifest a value as an INI-formatted string
    fn manifest_ini(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError {
                    span,
                    message: format!(
                        "std.manifestIni: expected object, got {}",
                        other.type_name()
                    ),
                    source_id,
                });
            }
        };

        let mut result = String::new();

        // Process "main" section (no header)
        let main_val = {
            let obj = self.memory_manager.load_object(obj_idx);
            obj.properties.iter().find_map(|(k, f)| {
                if self.memory_manager.load_string(*k) == "main"
                    && f.visibility != FieldVisibility::Hidden
                {
                    Some((f.value, f.super_obj))
                } else {
                    None
                }
            })
        };
        if let Some((main_raw, main_super)) = main_val {
            let main_forced = match main_raw {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), main_super)?,
                other => other,
            };
            let main_forced = self.force_value(main_forced)?;
            let main_obj_idx = match main_forced {
                Value::Object(idx) => idx,
                other => {
                    return Err(RuntimeError {
                        span,
                        message: format!(
                            "std.manifestIni: 'main' must be an object, got {}",
                            other.type_name()
                        ),
                        source_id,
                    });
                }
            };
            let main_fields: Vec<(String, Value, Option<ObjectIndex>)> = {
                let mobj = self.memory_manager.load_object(main_obj_idx);
                let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = mobj
                    .properties
                    .iter()
                    .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                    .map(|(k, f)| {
                        (
                            self.memory_manager.load_string(*k).to_string(),
                            f.value,
                            f.super_obj,
                        )
                    })
                    .collect();
                fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                fields
            };
            for (key, val, super_obj) in main_fields {
                let forced_val = match val {
                    Value::Closure(c) => {
                        self.execute_thunk_sync(c, Some(main_obj_idx), super_obj)?
                    }
                    other => other,
                };
                let forced_val = self.force_value(forced_val)?;
                let val_str = self.ini_scalar_to_string(forced_val, span.clone(), &source_id)?;
                result.push_str(&format!("{} = {}\n", key, val_str));
            }
        }

        // Process named sections
        let sections_val = {
            let obj = self.memory_manager.load_object(obj_idx);
            obj.properties.iter().find_map(|(k, f)| {
                if self.memory_manager.load_string(*k) == "sections"
                    && f.visibility != FieldVisibility::Hidden
                {
                    Some((f.value, f.super_obj))
                } else {
                    None
                }
            })
        };
        if let Some((sections_raw, sections_super)) = sections_val {
            let sections_forced = match sections_raw {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), sections_super)?,
                other => other,
            };
            let sections_forced = self.force_value(sections_forced)?;
            let sections_obj_idx = match sections_forced {
                Value::Object(idx) => idx,
                other => {
                    return Err(RuntimeError {
                        span,
                        message: format!(
                            "std.manifestIni: 'sections' must be an object, got {}",
                            other.type_name()
                        ),
                        source_id,
                    });
                }
            };
            let section_names: Vec<(String, Value, Option<ObjectIndex>)> = {
                let sobj = self.memory_manager.load_object(sections_obj_idx);
                let mut names: Vec<(String, Value, Option<ObjectIndex>)> = sobj
                    .properties
                    .iter()
                    .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                    .map(|(k, f)| {
                        (
                            self.memory_manager.load_string(*k).to_string(),
                            f.value,
                            f.super_obj,
                        )
                    })
                    .collect();
                names.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                names
            };
            for (section_name, section_raw, section_super) in section_names {
                result.push_str(&format!("[{}]\n", section_name));
                let section_forced = match section_raw {
                    Value::Closure(c) => {
                        self.execute_thunk_sync(c, Some(sections_obj_idx), section_super)?
                    }
                    other => other,
                };
                let section_forced = self.force_value(section_forced)?;
                let section_obj_idx = match section_forced {
                    Value::Object(idx) => idx,
                    other => {
                        return Err(RuntimeError {
                            span,
                            message: format!(
                                "std.manifestIni: section '{}' must be an object, got {}",
                                section_name,
                                other.type_name()
                            ),
                            source_id,
                        });
                    }
                };
                let section_fields: Vec<(String, Value, Option<ObjectIndex>)> = {
                    let sobj = self.memory_manager.load_object(section_obj_idx);
                    let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = sobj
                        .properties
                        .iter()
                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                        .map(|(k, f)| {
                            (
                                self.memory_manager.load_string(*k).to_string(),
                                f.value,
                                f.super_obj,
                            )
                        })
                        .collect();
                    fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                    fields
                };
                for (key, val, super_obj) in section_fields {
                    let forced_val = match val {
                        Value::Closure(c) => {
                            self.execute_thunk_sync(c, Some(section_obj_idx), super_obj)?
                        }
                        other => other,
                    };
                    let forced_val = self.force_value(forced_val)?;
                    let val_str =
                        self.ini_scalar_to_string(forced_val, span.clone(), &source_id)?;
                    result.push_str(&format!("{} = {}\n", key, val_str));
                }
            }
        }

        Ok(result)
    }

    fn ini_scalar_to_string(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: &str,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::String(idx) => Ok(self.memory_manager.load_string(idx).to_string()),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Null => Ok("null".to_string()),
            other => Err(RuntimeError {
                span,
                message: format!(
                    "std.manifestIni: value must be a scalar, got {}",
                    other.type_name()
                ),
                source_id: source_id.to_string(),
            }),
        }
    }

    /// Manifest a value as Python literal syntax
    fn manifest_python_value(
        &mut self,
        value: Value,
        depth: usize,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };

        match forced {
            Value::Null => Ok("None".to_string()),
            Value::Boolean(true) => Ok("True".to_string()),
            Value::Boolean(false) => Ok("False".to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestPython: cannot serialize NaN".to_string(),
                        source_id,
                    });
                }
                if n.is_infinite() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestPython: cannot serialize Infinite".to_string(),
                        source_id,
                    });
                }
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::String(s_idx) => {
                let s = self.memory_manager.load_string(s_idx).to_string();
                let mut out = String::from("\"");
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                Ok(out)
            }
            Value::Array(a_idx) => {
                let elements = self.memory_manager.load_array(a_idx).elements.clone();
                if elements.is_empty() {
                    return Ok("[ ]".to_string());
                }
                let indent = "   ".repeat(depth + 1);
                let close_indent = "   ".repeat(depth);
                let mut items = Vec::with_capacity(elements.len());
                for elem in elements {
                    let s = self.manifest_python_value(
                        elem,
                        depth + 1,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    items.push(format!("{}{}", indent, s));
                }
                Ok(format!("[\n{}\n{}]", items.join(",\n"), close_indent))
            }
            Value::Object(o_idx) => {
                let field_data: Vec<(String, Value, Option<ObjectIndex>)> = {
                    let obj = self.memory_manager.load_object(o_idx);
                    let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = obj
                        .properties
                        .iter()
                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                        .map(|(k, f)| {
                            (
                                self.memory_manager.load_string(*k).to_string(),
                                f.value,
                                f.super_obj,
                            )
                        })
                        .collect();
                    fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                    fields
                };

                if field_data.is_empty() {
                    return Ok("{ }".to_string());
                }

                let item_indent = "   ".repeat(depth + 1);
                let close_indent = "   ".repeat(depth);
                let mut pairs = Vec::with_capacity(field_data.len());
                for (key_str, field_val, super_obj) in field_data {
                    let forced_val = match field_val {
                        Value::Closure(c) => self.execute_thunk_sync(c, Some(o_idx), super_obj)?,
                        other => other,
                    };
                    let val_s = self.manifest_python_value(
                        forced_val,
                        depth + 1,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    // JSON-escape the key
                    let mut key_out = String::from("\"");
                    for ch in key_str.chars() {
                        match ch {
                            '"' => key_out.push_str("\\\""),
                            '\\' => key_out.push_str("\\\\"),
                            '\n' => key_out.push_str("\\n"),
                            '\r' => key_out.push_str("\\r"),
                            '\t' => key_out.push_str("\\t"),
                            c if (c as u32) < 0x20 => {
                                key_out.push_str(&format!("\\u{:04x}", c as u32));
                            }
                            c => key_out.push(c),
                        }
                    }
                    key_out.push('"');
                    pairs.push(format!("{}{}: {}", item_indent, key_out, val_s));
                }
                Ok(format!("{{\n{}\n{}}}", pairs.join(",\n"), close_indent))
            }
            _ => Err(RuntimeError {
                span,
                message: "std.manifestPython: cannot manifest function".to_string(),
                source_id,
            }),
        }
    }

    /// Manifest an object as Python variable assignments
    fn manifest_python_vars(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError {
                    span,
                    message: format!(
                        "std.manifestPythonVars: expected object, got {}",
                        other.type_name()
                    ),
                    source_id,
                });
            }
        };

        let fields: Vec<(String, Value, Option<ObjectIndex>)> = {
            let obj = self.memory_manager.load_object(obj_idx);
            let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = obj
                .properties
                .iter()
                .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                .map(|(k, f)| {
                    (
                        self.memory_manager.load_string(*k).to_string(),
                        f.value,
                        f.super_obj,
                    )
                })
                .collect();
            fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
            fields
        };

        let mut result = String::new();
        for (key, val, super_obj) in fields {
            let forced_val = match val {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), super_obj)?,
                other => other,
            };
            let python_val =
                self.manifest_python_value(forced_val, 0, span.clone(), source_id.clone())?;
            result.push_str(&format!("{} = {}\n", key, python_val));
        }
        Ok(result)
    }

    /// Manifest a value as a YAML document
    fn manifest_yaml_doc(
        &mut self,
        value: Value,
        depth: usize,
        indent_array_in_object: bool,
        quote_keys: bool,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };

        match forced {
            Value::Null => Ok("null".to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestYamlDoc: cannot serialize NaN".to_string(),
                        source_id,
                    });
                }
                if n.is_infinite() {
                    return Err(RuntimeError {
                        span,
                        message: "std.manifestYamlDoc: cannot serialize Infinite".to_string(),
                        source_id,
                    });
                }
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::String(s_idx) => {
                let s = self.memory_manager.load_string(s_idx).to_string();
                if yaml_needs_quoting(&s) {
                    let mut out = String::from("\"");
                    for ch in s.chars() {
                        match ch {
                            '"' => out.push_str("\\\""),
                            '\\' => out.push_str("\\\\"),
                            '\n' => out.push_str("\\n"),
                            '\r' => out.push_str("\\r"),
                            '\t' => out.push_str("\\t"),
                            c if (c as u32) < 0x20 => {
                                out.push_str(&format!("\\u{:04x}", c as u32));
                            }
                            c => out.push(c),
                        }
                    }
                    out.push('"');
                    Ok(out)
                } else {
                    Ok(s)
                }
            }
            Value::Array(a_idx) => {
                let elements = self.memory_manager.load_array(a_idx).elements.clone();
                if elements.is_empty() {
                    return Ok("[ ]".to_string());
                }
                let indent = "  ".repeat(depth);
                let mut lines = Vec::with_capacity(elements.len());
                for elem in elements {
                    let elem_str = self.manifest_yaml_doc(
                        elem,
                        depth + 1,
                        indent_array_in_object,
                        quote_keys,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    if elem_str.contains('\n') {
                        // Multi-line: first line on same line as "- ", rest indented
                        let mut sub_lines = elem_str.lines();
                        let first = sub_lines.next().unwrap_or("");
                        let rest: Vec<String> =
                            sub_lines.map(|l| format!("{}  {}", indent, l)).collect();
                        if rest.is_empty() {
                            lines.push(format!("{}- {}", indent, first));
                        } else {
                            lines.push(format!("{}- {}\n{}", indent, first, rest.join("\n")));
                        }
                    } else {
                        lines.push(format!("{}- {}", indent, elem_str));
                    }
                }
                Ok(lines.join("\n"))
            }
            Value::Object(o_idx) => {
                let field_data: Vec<(String, Value, Option<ObjectIndex>)> = {
                    let obj = self.memory_manager.load_object(o_idx);
                    let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = obj
                        .properties
                        .iter()
                        .filter(|(_, f)| f.visibility != FieldVisibility::Hidden)
                        .map(|(k, f)| {
                            (
                                self.memory_manager.load_string(*k).to_string(),
                                f.value,
                                f.super_obj,
                            )
                        })
                        .collect();
                    fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                    fields
                };

                if field_data.is_empty() {
                    return Ok("{ }".to_string());
                }

                let indent = "  ".repeat(depth);
                let mut lines = Vec::with_capacity(field_data.len());
                for (key_str, field_val, super_obj) in field_data {
                    let forced_val = match field_val {
                        Value::Closure(c) => self.execute_thunk_sync(c, Some(o_idx), super_obj)?,
                        other => other,
                    };
                    let key_repr = if quote_keys {
                        // Escape key as JSON string
                        let mut out = String::from("\"");
                        for ch in key_str.chars() {
                            match ch {
                                '"' => out.push_str("\\\""),
                                '\\' => out.push_str("\\\\"),
                                '\n' => out.push_str("\\n"),
                                '\r' => out.push_str("\\r"),
                                '\t' => out.push_str("\\t"),
                                c if (c as u32) < 0x20 => {
                                    out.push_str(&format!("\\u{:04x}", c as u32));
                                }
                                c => out.push(c),
                            }
                        }
                        out.push('"');
                        out
                    } else {
                        key_str.clone()
                    };

                    let val_str = self.manifest_yaml_doc(
                        forced_val,
                        depth + 1,
                        indent_array_in_object,
                        quote_keys,
                        span.clone(),
                        source_id.clone(),
                    )?;

                    if val_str.contains('\n') {
                        // Multi-line value: key on its own line then indented content
                        let indented: String = val_str
                            .lines()
                            .map(|l| format!("{}  {}", indent, l))
                            .collect::<Vec<_>>()
                            .join("\n");
                        lines.push(format!("{}{}:\n{}", indent, key_repr, indented));
                    } else {
                        lines.push(format!("{}{}: {}", indent, key_repr, val_str));
                    }
                }
                Ok(lines.join("\n"))
            }
            other => Err(RuntimeError {
                span,
                message: format!("std.manifestYamlDoc: cannot manifest {}", other.type_name()),
                source_id,
            }),
        }
    }

    fn manifest_yaml_stream(
        &mut self,
        value: Value,
        indent_array_in_object: bool,
        c_document_end: bool,
        quote_keys: bool,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };
        let arr_idx = match forced {
            Value::Array(idx) => idx,
            other => {
                return Err(RuntimeError {
                    span,
                    message: format!(
                        "manifestYamlStream: expected array, got {}",
                        other.type_name()
                    ),
                    source_id,
                });
            }
        };

        let elements = self.memory_manager.load_array(arr_idx).elements.clone();

        if elements.is_empty() {
            return Ok(String::new());
        }

        let mut parts: Vec<String> = Vec::new();
        for elem in elements {
            let doc = self.manifest_yaml_doc(
                elem,
                0,
                indent_array_in_object,
                quote_keys,
                span.clone(),
                source_id.clone(),
            )?;
            parts.push(format!("---\n{}", doc));
        }

        let mut result = parts.join("\n");
        if c_document_end {
            result.push_str("\n...");
        }
        Ok(result)
    }

    fn serde_yaml_to_jsonnet_value(
        &mut self,
        yaml: serde_yaml::Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        match yaml {
            serde_yaml::Value::Null => Ok(Value::Null),
            serde_yaml::Value::Bool(b) => Ok(Value::Boolean(b)),
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Ok(Value::Number(i as f64))
                } else if let Some(f) = n.as_f64() {
                    Ok(Value::Number(f))
                } else {
                    Err(RuntimeError {
                        span,
                        message: "parseYaml: unsupported number".to_string(),
                        source_id,
                    })
                }
            }
            serde_yaml::Value::String(s) => {
                let alloc = self.memory_manager.allocate_string(&s);
                Ok(Value::String(alloc.index))
            }
            serde_yaml::Value::Sequence(seq) => {
                let mut elements = Vec::new();
                for item in seq {
                    let val =
                        self.serde_yaml_to_jsonnet_value(item, span.clone(), source_id.clone())?;
                    elements.push(val);
                }
                let alloc = self.memory_manager.allocate_array(elements);
                Ok(Value::Array(alloc.index))
            }
            serde_yaml::Value::Mapping(map) => {
                let mut props = std::collections::HashMap::new();
                for (k, v) in map {
                    let key = match k {
                        serde_yaml::Value::String(s) => s,
                        other => format!("{:?}", other),
                    };
                    let key_idx = self.memory_manager.allocate_string(&key).index;
                    let val =
                        self.serde_yaml_to_jsonnet_value(v, span.clone(), source_id.clone())?;
                    props.insert(
                        key_idx,
                        ObjectField {
                            value: val,
                            super_obj: None,
                            visibility: FieldVisibility::Visible,
                        },
                    );
                }
                let alloc = self.memory_manager.allocate_object_with_properties(props);
                Ok(Value::Object(alloc.index))
            }
            serde_yaml::Value::Tagged(tagged) => {
                self.serde_yaml_to_jsonnet_value(tagged.value, span, source_id)
            }
        }
    }

    fn manifest_xml_jsonml(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };
        match forced {
            Value::String(idx) => {
                let s = self.memory_manager.load_string(idx).to_string();
                Ok(xml_escape(&s))
            }
            Value::Array(arr_idx) => {
                let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                if elements.is_empty() {
                    return Err(RuntimeError {
                        span,
                        message:
                            "manifestXmlJsonml: array must have at least one element (tag name)"
                                .to_string(),
                        source_id,
                    });
                }
                // First element: tag name
                let tag_val = self.force_value(elements[0])?;
                let tag_val = match tag_val {
                    Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                    other => other,
                };
                let tag = match tag_val {
                    Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                    other => {
                        return Err(RuntimeError {
                            span,
                            message: format!(
                                "manifestXmlJsonml: first element must be string tag, got {}",
                                other.type_name()
                            ),
                            source_id,
                        });
                    }
                };

                let mut attrs = String::new();
                let mut child_start = 1;

                // Check if second element is an attribute object
                if elements.len() > 1 {
                    let second_val = self.force_value(elements[1])?;
                    let second_val = match second_val {
                        Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                        other => other,
                    };
                    if let Value::Object(obj_idx) = second_val {
                        child_start = 2;
                        let fields: Vec<(String, Value)> = {
                            let obj = self.memory_manager.load_object(obj_idx);
                            let mut f: Vec<(String, Value)> = obj
                                .properties
                                .iter()
                                .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                                .map(|(k, field)| {
                                    (self.memory_manager.load_string(*k).to_string(), field.value)
                                })
                                .collect();
                            f.sort_by(|a, b| a.0.cmp(&b.0));
                            f
                        };
                        for (k, v) in fields {
                            let forced_v = self.force_value(v)?;
                            let forced_v = match forced_v {
                                Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                                other => other,
                            };
                            let vs = match forced_v {
                                Value::String(idx) => {
                                    self.memory_manager.load_string(idx).to_string()
                                }
                                Value::Number(n) => {
                                    if n.fract() == 0.0 {
                                        format!("{}", n as i64)
                                    } else {
                                        format!("{}", n)
                                    }
                                }
                                Value::Boolean(b) => b.to_string(),
                                other => {
                                    return Err(RuntimeError {
                                        span: span.clone(),
                                        message: format!(
                                            "manifestXmlJsonml: attribute value must be scalar, got {}",
                                            other.type_name()
                                        ),
                                        source_id: source_id.clone(),
                                    });
                                }
                            };
                            attrs.push_str(&format!(" {}=\"{}\"", k, xml_escape(&vs)));
                        }
                    }
                }

                // Children
                let mut children = String::new();
                for child in elements[child_start..].iter().copied() {
                    let child_str =
                        self.manifest_xml_jsonml(child, span.clone(), source_id.clone())?;
                    children.push_str(&child_str);
                }

                Ok(format!("<{}{}>{}</{}>", tag, attrs, children, tag))
            }
            other => Err(RuntimeError {
                span,
                message: format!(
                    "manifestXmlJsonml: expected string or array, got {}",
                    other.type_name()
                ),
                source_id,
            }),
        }
    }

    fn manifest_toml_ex(
        &mut self,
        value: Value,
        indent: &str,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError {
                    span,
                    message: format!("manifestTomlEx: expected object, got {}", other.type_name()),
                    source_id,
                });
            }
        };
        let indent_owned = indent.to_string();
        self.manifest_toml_table(obj_idx, &indent_owned, &[], span, source_id)
    }

    fn manifest_toml_table(
        &mut self,
        obj_idx: ObjectIndex,
        indent: &str,
        path: &[String],
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let fields: Vec<(String, Value, Option<ObjectIndex>)> = {
            let obj = self.memory_manager.load_object(obj_idx);
            let mut f: Vec<(String, Value, Option<ObjectIndex>)> = obj
                .properties
                .iter()
                .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                .map(|(k, field)| {
                    (
                        self.memory_manager.load_string(*k).to_string(),
                        field.value,
                        field.super_obj,
                    )
                })
                .collect();
            f.sort_by(|a, b| a.0.cmp(&b.0));
            f
        };

        let mut scalars = String::new();
        let mut tables = String::new();
        let mut array_tables = String::new();

        for (key, val, super_obj) in fields {
            let forced = match val {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), super_obj)?,
                other => self.force_value(other)?,
            };
            let forced = match forced {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), super_obj)?,
                other => other,
            };
            match forced {
                Value::Object(sub_obj_idx) => {
                    let mut sub_path = path.to_vec();
                    sub_path.push(key.clone());
                    let path_str = sub_path.join(".");
                    let indent_owned = indent.to_string();
                    let sub_content = self.manifest_toml_table(
                        sub_obj_idx,
                        &indent_owned,
                        &sub_path,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    tables.push_str(&format!("\n[{}]\n{}", path_str, sub_content));
                }
                Value::Array(arr_idx) => {
                    let elems = self.memory_manager.load_array(arr_idx).elements.clone();
                    let is_array_of_objects = if elems.is_empty() {
                        false
                    } else {
                        let first_forced = self.force_value(elems[0])?;
                        let first_forced = match first_forced {
                            Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                            other => other,
                        };
                        matches!(first_forced, Value::Object(_))
                    };
                    if is_array_of_objects {
                        let mut sub_path = path.to_vec();
                        sub_path.push(key.clone());
                        let path_str = sub_path.join(".");
                        for elem in &elems {
                            let elem_forced = self.force_value(*elem)?;
                            let elem_forced = match elem_forced {
                                Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                                other => other,
                            };
                            let sub_obj_idx = match elem_forced {
                                Value::Object(idx) => idx,
                                other => {
                                    return Err(RuntimeError {
                                        span: span.clone(),
                                        message: format!(
                                            "manifestTomlEx: mixed arrays not supported, got {}",
                                            other.type_name()
                                        ),
                                        source_id: source_id.clone(),
                                    });
                                }
                            };
                            let indent_owned = indent.to_string();
                            let sub_content = self.manifest_toml_table(
                                sub_obj_idx,
                                &indent_owned,
                                &sub_path,
                                span.clone(),
                                source_id.clone(),
                            )?;
                            array_tables.push_str(&format!("\n[[{}]]\n{}", path_str, sub_content));
                        }
                    } else {
                        let inline = self.manifest_toml_inline_array(
                            arr_idx,
                            span.clone(),
                            source_id.clone(),
                        )?;
                        scalars.push_str(&format!("{} = {}\n", key, inline));
                    }
                }
                scalar => {
                    let val_str =
                        self.manifest_toml_scalar(scalar, span.clone(), source_id.clone())?;
                    scalars.push_str(&format!("{} = {}\n", key, val_str));
                }
            }
        }

        Ok(format!("{}{}{}", scalars, tables, array_tables))
    }

    fn manifest_toml_scalar(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::Null => Err(RuntimeError {
                span,
                message: "manifestTomlEx: null values are not supported in TOML".to_string(),
                source_id,
            }),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::String(idx) => {
                let s = self.memory_manager.load_string(idx).to_string();
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                Ok(format!("\"{}\"", escaped))
            }
            other => Err(RuntimeError {
                span,
                message: format!(
                    "manifestTomlEx: unexpected value type {}",
                    other.type_name()
                ),
                source_id,
            }),
        }
    }

    fn manifest_toml_inline_array(
        &mut self,
        arr_idx: chunk::ArrayIndex,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let elems = self.memory_manager.load_array(arr_idx).elements.clone();
        let mut parts = Vec::new();
        for elem in elems {
            let forced = self.force_value(elem)?;
            let forced = match forced {
                Value::Closure(c) => self.execute_thunk_sync(c, None, None)?,
                other => other,
            };
            let s = self.manifest_toml_scalar(forced, span.clone(), source_id.clone())?;
            parts.push(s);
        }
        Ok(format!("[{}]", parts.join(", ")))
    }
}

/// Returns true if a YAML string value needs to be quoted
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn yaml_needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let lower = s.to_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "yes" | "no" | "on" | "off"
    ) {
        return true;
    }
    // Starts with a special YAML character
    if let Some(first) = s.chars().next() {
        if ":{[],&*?|-<>=!%@`'\"".contains(first) {
            return true;
        }
    }
    // Contains ': ' or ' #' (YAML comment/mapping indicators in flow context)
    if s.contains(": ") || s.contains(" #") {
        return true;
    }
    // Looks like a number
    if s.parse::<f64>().is_ok() {
        return true;
    }
    false
}

/// Main execution function - entry point for running Jsonnet bytecode
pub fn execute(
    chunk: Chunk,
    memory_manager: MemoryManager,
) -> Result<serde_json::Value, RuntimeError> {
    execute_with_ext_vars(chunk, memory_manager, &[])
}

/// Execute with external variables set via --ext-str / --ext-code CLI flags
pub fn execute_with_ext_vars(
    chunk: Chunk,
    memory_manager: MemoryManager,
    ext_strs: &[(String, String)],
) -> Result<serde_json::Value, RuntimeError> {
    let mut vm = VirtualMachine::new(chunk, memory_manager);

    for (k, v) in ext_strs {
        vm.set_ext_var_string(k, v);
    }

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

    #[test]
    fn test_ext_var_string() {
        // Compile std.extVar("myVar") and run with ext var set
        let source = r#"std.extVar("myVar")"#;
        let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler
            .compile(&mut memory_manager)
            .expect("compile failed");

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        vm.set_ext_var_string("myVar", "hello");
        let result = vm.interpret().expect("interpret failed");

        match result {
            Value::String(idx) => {
                assert_eq!(vm.memory_manager.load_string(idx), "hello");
            }
            other => panic!("expected string, got {:?}", other),
        }
    }

    #[test]
    fn test_ext_var_undefined_error() {
        // Compile std.extVar("undefined") and run without setting any ext vars
        let source = r#"std.extVar("undefined")"#;
        let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler
            .compile(&mut memory_manager)
            .expect("compile failed");

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret();

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("Undefined external variable"),
            "expected 'Undefined external variable' in error message"
        );
    }
}
