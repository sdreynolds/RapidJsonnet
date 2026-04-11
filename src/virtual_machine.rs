use chunk::{
    Chunk, ClosureIndex, FieldVisibility, I32_SIZE_BYTES, OPCODE_SIZE_BYTES, ObjectIndex, Opcode,
    OwnedChunk, RuntimeError, StringIndex, UpvalueIndex, Value,
};
use compiler;
use memory_manager::{MemoryManager, ObjectField};
use scanner;
use serialized_chunk;
use std::ops::Range;

use coverage::CoverageCollector;
use native::{self, call_native};

extern crate serde_yaml;

/// Maximum number of nested function calls
const MAX_FRAMES: usize = 1024;

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
    /// The field name this thunk is being evaluated for (for dynamic +: overrides)
    pub field_name: Option<StringIndex>,
    /// When set, the return value of this frame should be cached in the VM's field_cache
    /// under (object_index, field_key, self_obj).
    pub cache_target: Option<(ObjectIndex, StringIndex, ObjectIndex)>,
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
            field_name: None,
            cache_target: None,
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
    /// Pending field name for the next thunk call (consumed by call_closure)
    pending_field_name: Option<StringIndex>,
    /// Pending cache target for the next field thunk call (consumed by call_closure)
    pending_cache_target: Option<(ObjectIndex, StringIndex, ObjectIndex)>,
    /// Cached std object (created on first LoadStd, reused after)
    std_object: Option<Value>,
    /// Library search paths for import resolution (like -J / --jpath)
    jpaths: Vec<String>,
    /// Optional coverage collector for span tracking during test runs
    coverage_collector: Option<CoverageCollector>,
    /// Cache for evaluated object field thunks, keyed by (object, field, self).
    /// The self component is needed because `super.field` evaluates the thunk with
    /// the merged object as self, not the super object.
    field_cache: std::collections::HashMap<(ObjectIndex, StringIndex, ObjectIndex), Value>,
}

impl VirtualMachine {
    /// Create a new virtual machine with the given starting chunk and string pool
    pub fn new(chunk: Chunk, memory_manager: MemoryManager) -> Self {
        Self::new_from_owned(chunk.into_owned(), memory_manager)
    }

    /// Create a new virtual machine from a pre-compiled OwnedChunk
    pub fn new_from_owned(owned_chunk: OwnedChunk, mut memory_manager: MemoryManager) -> Self {
        // Create a top-level function from the chunk
        let func_result = memory_manager.allocate_function(None, 0, 0, owned_chunk);

        // Root the function so it survives GC during closure allocation
        memory_manager.push_external_roots(vec![Value::Function(func_result.index)], Vec::new());

        // Create a closure wrapping the top-level function
        let closure_result = memory_manager.allocate_closure(func_result.index, Vec::new());

        memory_manager.pop_external_roots();

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
            pending_field_name: None,
            pending_cache_target: None,
            std_object: None,
            jpaths: Vec::new(),
            coverage_collector: None,
            field_cache: std::collections::HashMap::new(),
        }
    }

    /// Set library search paths for import resolution
    pub fn set_jpaths(&mut self, jpaths: Vec<String>) {
        self.jpaths = jpaths;
    }

    /// Enable span-level coverage collection.
    pub fn enable_coverage(&mut self) {
        self.coverage_collector = Some(CoverageCollector::new());
    }

    /// Extract collected coverage data, leaving None in its place.
    pub fn take_coverage(&mut self) -> Option<CoverageCollector> {
        self.coverage_collector.take()
    }

    /// Get a reference to the memory manager (for test runner field inspection).
    pub fn memory_manager(&self) -> &MemoryManager {
        &self.memory_manager
    }

    /// Push external GC roots to protect values from garbage collection.
    /// Used by the test runner to keep the top-level object alive across test calls.
    pub fn push_external_roots(&mut self, roots: Vec<Value>) {
        self.memory_manager.push_external_roots(roots, Vec::new());
    }

    /// Pop the last set of external GC roots.
    pub fn pop_external_roots(&mut self) {
        self.memory_manager.pop_external_roots();
    }

    /// Resolve an import path: try relative to the importing file first,
    /// then fall back to searching each JPATH directory.
    fn resolve_import_path(&self, import_path: &str) -> String {
        let current_dir = std::path::Path::new(&self.current_chunk().source_id)
            .parent()
            .unwrap_or(std::path::Path::new(""));
        let relative_path = current_dir.join(import_path);

        // Try relative to the importing file first
        if relative_path.exists() {
            return relative_path.to_string_lossy().to_string();
        }

        // Fall back to JPATH search
        for jpath in &self.jpaths {
            let candidate = std::path::Path::new(jpath).join(import_path);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }

        // Default: return the relative path (will produce a "file not found" error later)
        relative_path.to_string_lossy().to_string()
    }

    /// Set an external variable as a string value (for --ext-str CLI flag)
    pub fn set_ext_var_string(&mut self, key: &str, value: &str) {
        let alloc = self.memory_manager.allocate_string(value);
        self.ext_vars
            .insert(key.to_string(), Value::String(alloc.index));
    }

    /// Set an external variable from Jsonnet code (for --ext-code CLI flag).
    /// Parses the code as a JSON value and converts it to a Jsonnet value.
    pub fn set_ext_var_code(&mut self, key: &str, code: &str) -> Result<(), RuntimeError> {
        // Try to parse as JSON first (covers most ext-code use cases)
        match serde_json::from_str::<serde_json::Value>(code) {
            Ok(json_val) => {
                let val = self.json_to_jsonnet_value(&json_val)?;
                self.ext_vars.insert(key.to_string(), val);
                Ok(())
            }
            Err(_) => {
                // Not valid JSON — try as a Jsonnet expression by writing to temp file
                // and using the import mechanism
                let temp_path = format!("/tmp/jsonnet_ext_code_{}.jsonnet", key);
                std::fs::write(&temp_path, code).map_err(|e| {
                    RuntimeError::new(
                        0..0,
                        format!("Failed to write ext-code temp file: {}", e),
                        "<main>".to_string(),
                    )
                })?;
                let import_alloc = self.memory_manager.allocate_import(&temp_path);
                let import_val = Value::Import(import_alloc.index);
                self.ext_vars.insert(key.to_string(), import_val);
                // Push as permanent external root so it survives compiler-triggered GC
                // (the compiler's GC only roots compiler state + external_roots)
                self.memory_manager
                    .push_external_roots(vec![import_val], Vec::new());
                if import_alloc.should_garbage_collect {
                    self.run_garbage_collection();
                }
                Ok(())
            }
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
            return Err(RuntimeError::new(
                self.get_current_span(),
                "Stack overflow - maximum stack size exceeded".to_string(),
                self.current_chunk().source_id.to_string(),
            ));
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
        self.stack.pop().ok_or_else(|| {
            RuntimeError::new(
                self.get_current_span(),
                "Stack underflow - attempted to pop from empty stack".to_string(),
                self.current_chunk().source_id.to_string(),
            )
        })
    }

    /// Pop a value and immediately force it if it's an Import Thunk
    fn pop_forced(&mut self) -> Result<Value, RuntimeError> {
        let val = self.pop()?;
        self.force_value(val)
    }

    /// Peek at the top value without popping
    fn peek(&self) -> Result<&Value, RuntimeError> {
        self.stack.last().ok_or_else(|| {
            RuntimeError::new(
                self.get_current_span(),
                "Stack underflow - attempted to peek empty stack".to_string(),
                self.current_chunk().source_id.to_string(),
            )
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
        let operand = chunk.read_u8(frame.ip + 1).ok_or_else(|| {
            RuntimeError::new(
                self.get_current_span(),
                "Invalid bytecode - missing operand".to_string(),
                chunk.source_id.to_string(),
            )
        })?;

        self.current_frame_mut().ip += 2; // opcode + 1 byte for u8
        Ok(operand)
    }

    /// Read a u16 operand from the current position and advance PC
    fn read_u16_operand(&mut self) -> Result<u16, RuntimeError> {
        let frame = self.current_frame();
        let chunk = self.current_chunk();
        let operand = chunk.read_u16(frame.ip + 1).ok_or_else(|| {
            RuntimeError::new(
                self.get_current_span(),
                "Invalid bytecode - missing operand".to_string(),
                chunk.source_id.to_string(),
            )
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
            .ok_or_else(|| {
                RuntimeError::new(
                    self.get_current_span(),
                    "Invalid bytecode - missing i32 operand".to_string(),
                    chunk.source_id.to_string(),
                )
            })?;

        self.current_frame_mut().ip += OPCODE_SIZE_BYTES + I32_SIZE_BYTES;
        Ok(operand)
    }

    /// Advance program counter by 1 (for opcodes with no operands)
    fn advance_pc(&mut self) {
        self.current_frame_mut().ip += 1;
    }

    /// Execute a thunk (field closure) synchronously and return its result.
    /// Force all closure field values in an object to their evaluated results.

    /// Force a lazy thunk (0-arg closure from a local binding).
    fn force_thunk(&mut self, closure_index: ClosureIndex) -> Result<Value, RuntimeError> {
        self.push(Value::Closure(closure_index))?;
        let target_frame_count = self.frame_count;
        self.call_closure(closure_index, 0, None, None)?;
        self.interpret_until(target_frame_count)
    }

    /// Check all assertions on an object. Each assertion is a closure that takes
    /// (self, super) and should return a truthy value or error.
    /// After checking, assertions are cleared so they only run once.
    fn check_object_assertions(&mut self, obj_idx: chunk::ObjectIndex) -> Result<(), RuntimeError> {
        // Walk the base_object chain to collect all assertions
        let mut all_assertions = Vec::new();
        let mut curr = Some(obj_idx);
        while let Some(idx) = curr {
            let obj = self.memory_manager.load_object(idx);
            all_assertions.extend(obj.assertions.clone());
            curr = obj.base_object;
        }
        if all_assertions.is_empty() {
            return Ok(());
        }
        // Clear assertions in all chain nodes so they only run once
        let mut curr = Some(obj_idx);
        while let Some(idx) = curr {
            let base = self.memory_manager.load_object(idx).base_object;
            if let Some(obj) = self.memory_manager.get_object_mut(idx) {
                obj.assertions.clear();
            }
            curr = base;
        }
        for closure_idx in all_assertions {
            self.execute_thunk_sync(closure_idx, Some(obj_idx), None)?;
        }
        Ok(())
    }

    /// Force a single array element if it's a thunk, caching the result.
    fn force_array_element(
        &mut self,
        array_key: chunk::ArrayIndex,
        index: usize,
        element: Value,
    ) -> Result<Value, RuntimeError> {
        if let Value::Closure(ci) = element {
            if self.memory_manager.load_closure(ci).is_thunk {
                let result = self.force_thunk(ci)?;
                // Cache the forced value back into the array
                self.memory_manager.load_array_mut(array_key).elements[index] = result;
                return Ok(result);
            }
        }
        if let Value::NativeThunk(_) = element {
            let result = self.force_value(element)?;
            // Cache the forced value back into the array
            self.memory_manager.load_array_mut(array_key).elements[index] = result;
            return Ok(result);
        }
        Ok(element)
    }

    /// Force all elements of an array, caching results.
    /// Force all elements of an array, caching results. Recurses into nested arrays.
    fn force_all_array_elements(
        &mut self,
        array_key: chunk::ArrayIndex,
    ) -> Result<(), RuntimeError> {
        // Root the array to protect from GC during thunk forcing
        self.memory_manager
            .external_roots
            .push(vec![Value::Array(array_key)]);
        let len = self.memory_manager.load_array(array_key).elements.len();
        for i in 0..len {
            let element = self.memory_manager.load_array(array_key).elements[i];
            let forced = self.force_array_element(array_key, i, element)?;
            // Recursively force nested arrays
            if let Value::Array(nested_idx) = forced {
                self.force_all_array_elements(nested_idx)?;
            }
        }
        self.memory_manager.external_roots.pop();
        Ok(())
    }

    /// Force all non-hidden fields of an object recursively through its inheritance chain.
    fn force_all_object_fields(
        &mut self,
        object_key: chunk::ObjectIndex,
    ) -> Result<(), RuntimeError> {
        // Root the object to protect from GC
        self.memory_manager
            .external_roots
            .push(vec![Value::Object(object_key)]);

        let mut curr = Some(object_key);
        while let Some(node_idx) = curr {
            let (keys, next_base) = {
                let obj = self.memory_manager.load_object(node_idx);
                (
                    obj.properties.keys().cloned().collect::<Vec<_>>(),
                    obj.base_object,
                )
            };

            for key in keys {
                let field_val = self.memory_manager.load_object(node_idx).properties[&key].value;
                if let Value::Closure(ci) = field_val {
                    if self.memory_manager.load_closure(ci).is_thunk {
                        let result = self.execute_thunk_sync_with_field(
                            ci,
                            Some(object_key),
                            next_base,
                            Some(key),
                        )?;
                        // Write back result to cache it in the object property
                        let obj = self.memory_manager.get_object_mut(node_idx).unwrap();
                        obj.properties.get_mut(&key).unwrap().value = result;
                    }
                }
            }
            curr = next_base;
        }

        self.memory_manager.external_roots.pop();
        Ok(())
    }

    fn execute_thunk_sync(
        &mut self,
        closure_index: ClosureIndex,
        self_obj: Option<ObjectIndex>,
        super_obj: Option<ObjectIndex>,
    ) -> Result<Value, RuntimeError> {
        self.execute_thunk_sync_with_field(closure_index, self_obj, super_obj, None)
    }

    /// Execute a thunk with an optional field name for LoadFieldName support.
    fn execute_thunk_sync_with_field(
        &mut self,
        closure_index: ClosureIndex,
        self_obj: Option<ObjectIndex>,
        super_obj: Option<ObjectIndex>,
        field_name: Option<StringIndex>,
    ) -> Result<Value, RuntimeError> {
        self.push(Value::Closure(closure_index))?;
        self.push(self_obj.map(Value::Object).unwrap_or(Value::Null))?;
        self.push(super_obj.map(Value::Object).unwrap_or(Value::Null))?;
        let target_frame_count = self.frame_count;
        self.pending_field_name = field_name;
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
                self.call_native_checked(id, &[arg], span, source_id)
            }
            _ => Err(RuntimeError::new(
                self.get_current_span(),
                format!("keyF argument must be a function, got {:?}", func),
                self.current_chunk().source_id.to_string(),
            )),
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
                self.call_native_checked(id, &[arg1, arg2], span, source_id)
            }
            _ => Err(RuntimeError::new(
                self.get_current_span(),
                "expected function as callback".to_string(),
                self.current_chunk().source_id.to_string(),
            )),
        }
    }

    /// Call a native function, intercepting `std.assertEqual` so it uses the VM's
    /// `values_equal` (which evaluates thunks) instead of the native `values_equal`
    /// (which compares raw closure references).
    fn call_native_checked(
        &mut self,
        id: chunk::NativeFuncId,
        args: &[Value],
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        // Root args to protect from GC — args may not be on the stack
        self.memory_manager.external_roots.push(args.to_vec());

        let result = self.call_native_checked_inner(id, args, span, source_id);

        self.memory_manager.external_roots.pop();
        result
    }

    fn call_native_checked_inner(
        &mut self,
        id: chunk::NativeFuncId,
        args: &[Value],
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        if id == chunk::NativeFuncId::Trace && args.len() == 2 {
            let msg = match args[0] {
                Value::String(s) => self.memory_manager.load_string(s).to_string(),
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.trace() first argument must be a string".to_string(),
                        source_id,
                    ));
                }
            };
            // Compute file:line from source_id and span
            let filename = std::path::Path::new(&source_id)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| source_id.clone());
            let line = std::fs::read_to_string(&source_id)
                .map(|content| {
                    content[..span.start.min(content.len())]
                        .chars()
                        .filter(|&c| c == '\n')
                        .count()
                        + 1
                })
                .unwrap_or(0);
            if msg.is_empty() {
                eprintln!("TRACE: {}:{} ", filename, line);
            } else {
                eprintln!("TRACE: {}:{} {}", filename, line, msg);
            }
            return Ok(args[1]);
        } else if id == chunk::NativeFuncId::ToString && args.len() == 1 {
            // Intercept std.toString so we can handle objects/arrays
            // (requires VM to force thunks in object fields)
            if matches!(args[0], Value::String(_)) {
                return Ok(args[0]);
            }
            let s = self.value_to_string_for_concat(&args[0])?;
            let alloc = self.memory_manager.allocate_string(&s);
            Ok(Value::String(alloc.index))
        } else if id == chunk::NativeFuncId::AssertEqual && args.len() == 2 {
            // Force both args before comparing (they may be imports or thunks)
            let a = self.force_value(args[0])?;
            let b = self.force_value(args[1])?;
            if self.values_equal(&a, &b)? {
                Ok(Value::Boolean(true))
            } else {
                let a_display = native::display_value(a, &self.memory_manager);
                let b_display = native::display_value(b, &self.memory_manager);
                Err(RuntimeError::new(
                    span,
                    format!(
                        "Assertion failed: {} was not equal to {}",
                        a_display, b_display
                    ),
                    source_id,
                ))
            }
        } else if id == chunk::NativeFuncId::Length && args.len() == 1 {
            // Handle std.length for functions (returns arity)
            match args[0] {
                Value::Closure(c) => {
                    let func_idx = self.memory_manager.load_closure(c).function;
                    let arity = self.memory_manager.load_function(func_idx).arity;
                    Ok(Value::Number(arity as f64))
                }
                Value::Function(f) => {
                    let arity = self.memory_manager.load_function(f).arity;
                    Ok(Value::Number(arity as f64))
                }
                Value::NativeFunction(id) => Ok(Value::Number(id.arity() as f64)),
                _ => call_native(id, args, &mut self.memory_manager, span, source_id),
            }
        } else if id == chunk::NativeFuncId::MakeArray && args.len() == 2 {
            let sz_val = args[0];
            let func_val = args[1];
            let sz = match sz_val {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "std.makeArray expected positive integer for size, got {:?}",
                            sz_val
                        ),
                        source_id,
                    ));
                }
            };
            // Build lazy NativeThunks: each stores (func, i) and is forced on demand.
            // This avoids creating 3 GC objects (Chunk + ManagedFunction + ManagedClosure)
            // per element — only one ManagedNativeThunk is needed.
            let mut elements = Vec::with_capacity(sz);
            let mut should_gc = false;
            for i in 0..sz {
                let thunk_alloc = self
                    .memory_manager
                    .allocate_native_thunk(func_val, Value::Number(i as f64));
                should_gc |= thunk_alloc.should_garbage_collect;
                elements.push(Value::NativeThunk(thunk_alloc.index));
            }
            self.memory_manager
                .push_external_roots(elements.clone(), Vec::new());
            let alloc = self.memory_manager.allocate_array(elements);
            self.memory_manager.pop_external_roots();
            if should_gc || alloc.should_garbage_collect {
                let result_val = Value::Array(alloc.index);
                self.memory_manager
                    .push_external_roots(vec![result_val], Vec::new());
                self.run_garbage_collection();
                self.memory_manager.pop_external_roots();
            }
            Ok(Value::Array(alloc.index))
        } else if id == chunk::NativeFuncId::ManifestYamlDoc && args.len() == 3 {
            let value = args[0];
            let indent_array_in_object = match args[1] {
                Value::Boolean(b) => b,
                Value::Null => false, // default
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestYamlDoc: indent_array_in_object must be bool".to_string(),
                        source_id,
                    ));
                }
            };
            let quote_keys = match args[2] {
                Value::Boolean(b) => b,
                Value::Null => true, // default
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestYamlDoc: quote_keys must be bool".to_string(),
                        source_id,
                    ));
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
            Ok(Value::String(idx.index))
        } else if id == chunk::NativeFuncId::ManifestYamlStream && args.len() == 4 {
            let value = args[0];
            let indent_array_in_object = match args[1] {
                Value::Boolean(b) => b,
                Value::Null => false,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestYamlStream: indent_array_in_object must be bool".to_string(),
                        source_id,
                    ));
                }
            };
            let c_document_end = match args[2] {
                Value::Boolean(b) => b,
                Value::Null => true,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestYamlStream: c_document_end must be bool".to_string(),
                        source_id,
                    ));
                }
            };
            let quote_keys = match args[3] {
                Value::Boolean(b) => b,
                Value::Null => true,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestYamlStream: quote_keys must be bool".to_string(),
                        source_id,
                    ));
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
            Ok(Value::String(idx.index))
        } else if id == chunk::NativeFuncId::ManifestJsonEx && args.len() >= 2 {
            let value = args[0];
            let indent_str = match args[1] {
                Value::String(s) => self.memory_manager.load_string(s).to_string(),
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "manifestJsonEx: indent must be string".to_string(),
                        source_id,
                    ));
                }
            };
            let newline = if args.len() > 2 {
                match args[2] {
                    Value::String(s) => self.memory_manager.load_string(s).to_string(),
                    _ => "\n".to_string(),
                }
            } else {
                "\n".to_string()
            };
            let key_val_sep = if args.len() > 3 {
                match args[3] {
                    Value::String(s) => self.memory_manager.load_string(s).to_string(),
                    _ => ": ".to_string(),
                }
            } else {
                ": ".to_string()
            };
            let result = self.manifest_json_value(
                value,
                &indent_str,
                &newline,
                &key_val_sep,
                0,
                span.clone(),
                &source_id,
            )?;
            let idx = self.memory_manager.allocate_string(&result);
            Ok(Value::String(idx.index))
        } else if id == chunk::NativeFuncId::ManifestJson {
            let value = args[0];
            let result =
                self.manifest_json_value(value, "   ", "\n", ": ", 0, span.clone(), &source_id)?;
            let result = Self::fix_manifest_json_empties(&result);
            let idx = self.memory_manager.allocate_string(&result);
            Ok(Value::String(idx.index))
        } else if id == chunk::NativeFuncId::ManifestJsonMinified {
            let value = args[0];
            let result =
                self.manifest_json_value(value, "", "", ":", 0, span.clone(), &source_id)?;
            let idx = self.memory_manager.allocate_string(&result);
            Ok(Value::String(idx.index))
        } else if matches!(
            id,
            chunk::NativeFuncId::MinArray | chunk::NativeFuncId::MaxArray
        ) {
            // MinArray/MaxArray need VM-level handling to call keyF
            self.handle_min_max_array(id, args, span, source_id)
        } else if id == chunk::NativeFuncId::ManifestIni
            || id == chunk::NativeFuncId::ManifestPython
            || id == chunk::NativeFuncId::ManifestPythonVars
            || id == chunk::NativeFuncId::ManifestTomlEx
            || id == chunk::NativeFuncId::ManifestXmlJsonml
        {
            // These manifest functions need VM-level handling for thunk forcing
            // Route to the appropriate handler
            self.handle_manifest_native(id, args, span, source_id)
        } else {
            // Force thunks inside array arguments before passing to native code,
            // except for functions that don't access array elements.
            if !matches!(
                id,
                chunk::NativeFuncId::Length | chunk::NativeFuncId::Reverse
            ) {
                self.force_array_args(args)?;
            }
            call_native(id, args, &mut self.memory_manager, span, source_id)
        }
    }

    /// Handle manifest native functions that need VM-level thunk forcing.
    fn handle_min_max_array(
        &mut self,
        id: chunk::NativeFuncId,
        args: &[Value],
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        let arr_val = args[0];
        let key_f = args.get(1).copied();
        let on_empty = args.get(2).copied();

        let arr_idx = match arr_val {
            Value::Array(a) => a,
            other => {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "std.{} expected array, got {}",
                        id.name(),
                        other.type_name()
                    ),
                    source_id,
                ));
            }
        };

        // Root args before forcing array elements (which may trigger GC)
        let mut roots = vec![arr_val];
        if let Some(kf) = key_f {
            roots.push(kf);
        }
        if let Some(oe) = on_empty {
            roots.push(oe);
        }
        self.memory_manager.external_roots.push(roots);
        self.force_all_array_elements(arr_idx)?;
        let elements: Vec<Value> = self.memory_manager.load_array(arr_idx).elements.clone();

        if elements.is_empty() {
            self.memory_manager.external_roots.pop();
            match on_empty {
                Some(v) if !matches!(v, Value::Null) => {
                    return Ok(v);
                }
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        format!("std.{}: empty array", id.name()),
                        source_id,
                    ));
                }
            }
        }

        // Check if keyF is null or absent
        let effective_key_f = match key_f {
            None | Some(Value::Null) => None,
            Some(v) => Some(v),
        };

        let result = if let Some(key_f_val) = effective_key_f {
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
                let ord = native::compare_values(key, best_key, &self.memory_manager);
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
            best_elem
        } else {
            // No keyF — compare elements directly
            let mut best = elements[0];
            for &elem in elements.iter().skip(1) {
                let ord = native::compare_values(elem, best, &self.memory_manager);
                let take = if id == chunk::NativeFuncId::MinArray {
                    ord == std::cmp::Ordering::Less
                } else {
                    ord == std::cmp::Ordering::Greater
                };
                if take {
                    best = elem;
                }
            }
            best
        };
        self.memory_manager.external_roots.pop(); // pop args roots
        Ok(result)
    }

    fn handle_manifest_native(
        &mut self,
        id: chunk::NativeFuncId,
        args: &[Value],
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        match id {
            chunk::NativeFuncId::ManifestIni => {
                let result = self.manifest_ini(args[0], span, source_id)?;
                let idx = self.memory_manager.allocate_string(&result);
                Ok(Value::String(idx.index))
            }
            chunk::NativeFuncId::ManifestPython => {
                let result = self.manifest_python_value(args[0], 0, span, source_id)?;
                let idx = self.memory_manager.allocate_string(&result);
                Ok(Value::String(idx.index))
            }
            chunk::NativeFuncId::ManifestPythonVars => {
                let result = self.manifest_python_vars(args[0], span, source_id)?;
                let idx = self.memory_manager.allocate_string(&result);
                Ok(Value::String(idx.index))
            }
            chunk::NativeFuncId::ManifestTomlEx => {
                let indent = match args[1] {
                    Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "manifestTomlEx: indent must be a string".to_string(),
                            source_id,
                        ));
                    }
                };
                let result = self.manifest_toml_ex(args[0], &indent, span, source_id)?;
                let idx = self.memory_manager.allocate_string(&result);
                Ok(Value::String(idx.index))
            }
            chunk::NativeFuncId::ManifestXmlJsonml => {
                let result = self.manifest_xml_jsonml(args[0], span, source_id)?;
                let idx = self.memory_manager.allocate_string(&result);
                Ok(Value::String(idx.index))
            }
            _ => call_native(id, args, &mut self.memory_manager, span, source_id),
        }
    }

    /// Force all thunk elements inside any array arguments.
    /// Native functions can't force thunks, so we do it before the call.
    fn force_array_args(&mut self, args: &[Value]) -> Result<(), RuntimeError> {
        for arg in args.iter() {
            if let Value::Array(arr_idx) = arg {
                self.force_all_array_elements(*arr_idx)?;
            }
        }
        Ok(())
    }

    /// Merge two objects (left + right) and return the resulting ObjectIndex.
    /// Used to build full super chains during multi-level inheritance.
    /// Keys present in any node's `deleted_keys` are treated as absent throughout the chain.
    pub fn get_object_field_resolution(
        &self,
        target_obj: ObjectIndex,
        key: StringIndex,
    ) -> Option<(ObjectField, ObjectIndex)> {
        let mut deleted = std::collections::HashSet::new();
        let mut curr = Some(target_obj);
        while let Some(obj_idx) = curr {
            let obj = self.memory_manager.load_object(obj_idx);
            if let Some(dk) = &obj.deleted_keys {
                deleted.extend(dk.iter().copied());
            }
            if !deleted.contains(&key) {
                if let Some(field) = obj.get_field(&key) {
                    return Some((field.clone(), obj_idx));
                }
            }
            curr = obj.base_object;
        }
        None
    }

    /// Enumerate all fields of an object by walking the base_object chain.
    /// Returns (key, value, super_obj_for_field, visibility) with shallower nodes winning on collision.
    /// Keys in any node's `deleted_keys` set are skipped at every level of the chain.
    fn enumerate_object_fields(
        &self,
        root: ObjectIndex,
    ) -> Vec<(StringIndex, Value, Option<ObjectIndex>, FieldVisibility)> {
        let mut seen = std::collections::HashSet::new();
        let mut deleted = std::collections::HashSet::new();
        let mut result = Vec::new();
        let mut curr = Some(root);
        while let Some(idx) = curr {
            let obj = self.memory_manager.load_object(idx);
            if let Some(dk) = &obj.deleted_keys {
                deleted.extend(dk.iter().copied());
            }
            let base = obj.base_object;
            for (key, field) in &obj.properties {
                if !deleted.contains(key) && seen.insert(*key) {
                    result.push((*key, field.value, base, field.visibility));
                }
            }
            curr = base;
        }
        result
    }

    /// Merge two objects (left + right) and return the resulting ObjectIndex.
    /// Used to build full super chains during multi-level inheritance.
    fn merge_objects(
        &mut self,
        left_key: ObjectIndex,
        right_key: ObjectIndex,
    ) -> Result<ObjectIndex, RuntimeError> {
        let mut chain = Vec::new();
        let mut curr = Some(right_key);
        while let Some(c) = curr {
            chain.push(c);
            curr = self.memory_manager.load_object(c).base_object;
        }

        let mut current_base = left_key;
        for &node_key in chain.iter().rev() {
            // Phase 1: extract raw field data (drops the borrow before phase 2)
            let (raw_fields, assertions): (Vec<(StringIndex, Value, FieldVisibility)>, Vec<_>) = {
                let obj = self.memory_manager.load_object(node_key);
                (
                    obj.properties
                        .iter()
                        .map(|(k, f)| (*k, f.value, f.visibility))
                        .collect(),
                    obj.assertions.clone(),
                )
            };

            // Phase 2: apply visibility inheritance per spec rule h_L + h_R:
            //   h_L + h_R = h_L  if h_R = ':'  (right uses ':' → inherit left's visibility)
            //   h_L + h_R = h_R  otherwise       (right explicitly sets hidden/force-visible)
            let mut properties = std::collections::HashMap::new();
            for (key, value, vis) in raw_fields {
                let final_vis = if vis == FieldVisibility::Visible {
                    // Right uses ':', so inherit from left chain if the field exists there
                    self.get_object_field_resolution(left_key, key)
                        .map(|(f, _)| f.visibility)
                        .unwrap_or(FieldVisibility::Visible)
                } else {
                    // Right explicitly sets :: or :::, use it directly
                    vis
                };
                properties.insert(key, ObjectField::new(value, final_vis));
            }

            // Phase 3: allocate merged node
            let new_obj_allocation = self.memory_manager.allocate_object_full_with_base(
                Some(current_base),
                properties,
                assertions,
            );
            current_base = new_obj_allocation.index;
            // The caller (Opcode::Add) will push the final result and can GC then.
        }

        Ok(current_base)
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

        // Validate arity: arg_count must be between required_params and arity
        let arity = function.arity as usize;
        let required = function.required_params as usize;
        if arg_count < required || arg_count > arity {
            if required == arity {
                return Err(RuntimeError::new(
                    self.get_current_span(),
                    format!("Function expects {} argument(s), got {}", arity, arg_count),
                    self.current_chunk().source_id.to_string(),
                ));
            } else {
                return Err(RuntimeError::new(
                    self.get_current_span(),
                    format!(
                        "Function expects {}-{} argument(s), got {}",
                        required, arity, arg_count
                    ),
                    self.current_chunk().source_id.to_string(),
                ));
            }
        }

        // Pad missing optional arguments with Uninitialized
        for _ in arg_count..arity {
            self.push(Value::Uninitialized)?;
        }
        let arg_count = arity; // Now we have the full arity on the stack

        // Check stack depth
        if self.frame_count >= MAX_FRAMES {
            return Err(RuntimeError::new(
                self.get_current_span(),
                format!(
                    "Stack overflow - exceeded maximum call depth of {}",
                    MAX_FRAMES
                ),
                self.current_chunk().source_id.to_string(),
            ));
        }

        // Calculate stack_base: points to the closure on the stack
        // Stack: [..., closure, arg0, arg1, ..., argN-1]
        //              ^stack_base                      ^stack top
        let stack_base = self.stack.len() - arg_count - 1;

        // Create new call frame
        let mut new_frame = CallFrame::new(closure_index, 0, stack_base, self_obj, super_obj);

        // Consume pending field name if set (for dynamic +: override thunks)
        new_frame.field_name = self.pending_field_name.take();
        new_frame.cache_target = self.pending_cache_target.take();

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
        match val {
            Value::Import(import_idx) => {
                // Check if already cached
                if let Some(cached) = self.memory_manager.load_import(import_idx).cached_result {
                    return Ok(cached);
                }

                // Detect cyclic imports
                let path_str = {
                    let import = self.memory_manager.load_import(import_idx);
                    if import.evaluating.get() {
                        let path_str = self.memory_manager.load_string(import.path).to_string();
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!(
                                "Cyclic import detected: file '{}' is already being evaluated",
                                path_str
                            ),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }
                    import.evaluating.set(true);
                    self.memory_manager.load_string(import.path).to_string()
                };

                // Protect the current VM roots from GC during nested compilation and execution
                let mut roots = Vec::from(self.stack.clone());
                roots.push(val); // Protect the value we are currently forcing
                for i in 0..self.frame_count {
                    roots.push(Value::Closure(self.frames[i].closure));
                }
                for ext_val in self.ext_vars.values() {
                    roots.push(*ext_val);
                }

                let mut open_upvalue_roots = Vec::new();
                let mut upvalue = self.open_upvalues;
                while let Some(upvalue_index) = upvalue {
                    open_upvalue_roots.push(upvalue_index);
                    upvalue = self.memory_manager.load_upvalue(upvalue_index).next;
                }

                self.memory_manager
                    .push_external_roots(roots, open_upvalue_roots);

                let compiled_path = format!("{}c", path_str);
                let owned_chunk = if std::path::Path::new(&compiled_path).exists() {
                    let bytes = match std::fs::read(&compiled_path) {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            self.memory_manager.pop_external_roots();
                            self.memory_manager
                                .load_import_mut(import_idx)
                                .evaluating
                                .set(false);
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Failed to read compiled file '{}': {}", compiled_path, e),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };
                    serialized_chunk::deserialize_program(&bytes, &mut self.memory_manager)
                } else {
                    let content = match std::fs::read_to_string(&path_str) {
                        Ok(content) => content,
                        Err(e) => {
                            self.memory_manager.pop_external_roots();
                            self.memory_manager
                                .load_import_mut(import_idx)
                                .evaluating
                                .set(false);
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Failed to read imported file '{}': {}", path_str, e),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };

                    let mut scanner = scanner::Scanner::new(&content, &path_str);
                    let compiler = compiler::Compiler::new(&mut scanner, &path_str);
                    match compiler.compile(&mut self.memory_manager) {
                        Ok(chunk) => chunk.into_owned(),
                        Err(e) => {
                            self.memory_manager.pop_external_roots();
                            self.memory_manager
                                .load_import_mut(import_idx)
                                .evaluating
                                .set(false);
                            return Err(RuntimeError {
                                span: self.get_current_span(),
                                message: format!("while evaluating import \"{}\"", path_str),
                                source_id: self.current_chunk().source_id.to_string(),
                                cause: Some(Box::new(e)),
                            });
                        }
                    }
                };

                let dummy_memory_manager = memory_manager::MemoryManager::new();
                let actual_memory_manager =
                    std::mem::replace(&mut self.memory_manager, dummy_memory_manager);
                let mut sub_vm = VirtualMachine::new_from_owned(owned_chunk, actual_memory_manager);
                sub_vm.jpaths = self.jpaths.clone();
                if self.coverage_collector.is_some() {
                    sub_vm.enable_coverage();
                }
                let result = sub_vm.interpret();

                if let Some(sub_coverage) = sub_vm.take_coverage() {
                    if let Some(parent_coverage) = &mut self.coverage_collector {
                        parent_coverage.merge(sub_coverage);
                    }
                }
                self.memory_manager = sub_vm.memory_manager;
                self.memory_manager.pop_external_roots();

                match result {
                    Ok(evaluated_value) => {
                        let forced = match self.force_value(evaluated_value) {
                            Ok(v) => v,
                            Err(e) => {
                                self.memory_manager
                                    .load_import_mut(import_idx)
                                    .evaluating
                                    .set(false);
                                return Err(RuntimeError {
                                    span: self.get_current_span(),
                                    message: format!("while evaluating import \"{}\"", path_str),
                                    source_id: self.current_chunk().source_id.to_string(),
                                    cause: Some(Box::new(e)),
                                });
                            }
                        };
                        let import = self.memory_manager.load_import_mut(import_idx);
                        import.cached_result = Some(forced);
                        import.evaluating.set(false);
                        Ok(forced)
                    }
                    Err(e) => {
                        self.memory_manager
                            .load_import_mut(import_idx)
                            .evaluating
                            .set(false);
                        Err(RuntimeError {
                            span: self.get_current_span(),
                            message: format!("while evaluating import \"{}\"", path_str),
                            source_id: self.current_chunk().source_id.to_string(),
                            cause: Some(Box::new(e)),
                        })
                    }
                }
            }
            Value::Closure(ci) if self.memory_manager.load_closure(ci).is_thunk => {
                let func = self
                    .memory_manager
                    .load_function(self.memory_manager.load_closure(ci).function);
                if func.arity == 0 {
                    self.force_thunk(ci)
                } else {
                    // It's a field thunk, we can't force it without object context (self, super).
                    // We return it as-is and expect the caller (e.g. value_to_json) to handle it.
                    Ok(val)
                }
            }
            Value::NativeThunk(thunk_idx) => {
                // Return cached result if already forced
                if let Some(cached) = self.memory_manager.load_native_thunk(thunk_idx).cached {
                    return Ok(cached);
                }
                // Load func and arg (copy out to avoid borrow checker issue)
                let (func, arg) = {
                    let t = self.memory_manager.load_native_thunk(thunk_idx);
                    (t.func, t.arg)
                };
                // Call func(arg) and cache the result
                let result = self.call_value_with_one_arg(func, arg)?;
                self.memory_manager.load_native_thunk_mut(thunk_idx).cached = Some(result);
                Ok(result)
            }
            _ => Ok(val),
        }
    }

    /// Main interpretation loop
    pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
        self.interpret_until(0)
    }

    /// Force a field thunk to get its actual value.
    /// Object field values are closures with arity=2 (self, super) that must be
    /// forced to obtain the real value. This mirrors the ObjectIndex opcode behavior.
    pub fn force_field_thunk(
        &mut self,
        closure_index: ClosureIndex,
        obj_index: ObjectIndex,
        super_obj: Option<ObjectIndex>,
    ) -> Result<Value, RuntimeError> {
        self.execute_thunk_sync(closure_index, Some(obj_index), super_obj)
    }

    /// Call a zero-argument closure and run it to completion.
    /// Used by the test runner to invoke individual test functions
    /// after force_field_thunk has yielded the function closure.
    pub fn call_test_closure(
        &mut self,
        closure_index: ClosureIndex,
    ) -> Result<Value, RuntimeError> {
        self.push(Value::Closure(closure_index))?;
        let target_frame_count = self.frame_count;
        self.call_closure(closure_index, 0, None, None)?;
        self.interpret_until(target_frame_count)
    }

    fn interpret_until(&mut self, target_frame_count: usize) -> Result<Value, RuntimeError> {
        loop {
            // Store the start IP of this instruction for error reporting
            self.instruction_start_ip = self.current_frame().ip;

            // Record span for coverage if enabled
            if self.coverage_collector.is_some() {
                let chunk = self.current_chunk();
                let span = chunk.get_span(self.instruction_start_ip).cloned();
                let source_id = chunk.source_id.to_string();
                if let (Some(collector), Some(span)) = (&mut self.coverage_collector, span) {
                    collector.record(&source_id, &span);
                }
            }

            let frame = self.current_frame();
            let chunk = self.current_chunk();

            // Check if we've reached the end
            if frame.ip >= chunk.count() {
                return Err(RuntimeError::new(
                    self.get_current_span(),
                    "Unexpected end of bytecode - missing Return instruction".to_string(),
                    chunk.source_id.to_string(),
                ));
            }

            let opcode = chunk.read_opcode(frame.ip).ok_or_else(|| {
                RuntimeError::new(
                    self.get_current_span(),
                    "Invalid opcode in bytecode".to_string(),
                    chunk.source_id.to_string(),
                )
            })?;

            match opcode {
                Opcode::LoadSelf => {
                    let self_obj = self.current_frame().self_obj.ok_or_else(|| {
                        RuntimeError::new(
                            self.get_current_span(),
                            "'self' used outside of object scope".to_string(),
                            self.current_chunk().source_id.to_string(),
                        )
                    })?;
                    self.push(Value::Object(self_obj))?;
                    self.advance_pc();
                }

                Opcode::LoadSuper => {
                    let super_obj = self.current_frame().super_obj.ok_or_else(|| {
                        RuntimeError::new(
                            self.get_current_span(),
                            "'super' used outside of object scope".to_string(),
                            self.current_chunk().source_id.to_string(),
                        )
                    })?;
                    self.push(Value::Object(super_obj))?;
                    self.advance_pc();
                }

                Opcode::LoadFieldName => {
                    let field_name = self.current_frame().field_name.ok_or_else(|| {
                        RuntimeError::new(
                            self.get_current_span(),
                            "LoadFieldName used outside of field thunk context".to_string(),
                            self.current_chunk().source_id.to_string(),
                        )
                    })?;
                    self.push(Value::String(field_name))?;
                    self.advance_pc();
                }

                Opcode::InOp => {
                    // Membership test: key in object
                    // Stack: [key, object] -> [bool]
                    let object_val = self.pop_forced()?;
                    let key_val = self.pop_forced()?;

                    match (key_val, object_val) {
                        (Value::String(key), Value::Object(obj_idx)) => {
                            let has_field =
                                self.get_object_field_resolution(obj_idx, key).is_some();
                            self.push(Value::Boolean(has_field))?;
                        }
                        (key, obj) => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!(
                                    "'in' operator requires (string, object), got ({:?}, {:?})",
                                    key, obj
                                ),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    }
                    self.advance_pc();
                }

                Opcode::SuperHasField => {
                    let field_name = self.pop_forced()?;
                    let super_obj_key = self.current_frame().super_obj;

                    let has_field = if let (Value::String(field_key), Some(super_key)) =
                        (field_name, super_obj_key)
                    {
                        self.get_object_field_resolution(super_key, field_key)
                            .is_some()
                    } else {
                        false
                    };

                    self.push(Value::Boolean(has_field))?;
                    self.advance_pc();
                }

                Opcode::SuperIndex => {
                    let field_name = self.pop_forced()?; // Pop property name

                    let (self_obj_key, super_obj_key) = {
                        let frame = self.current_frame();
                        let s = frame.self_obj.ok_or_else(|| {
                            RuntimeError::new(
                                self.get_current_span(),
                                "'super' used outside of object scope".to_string(),
                                self.current_chunk().source_id.to_string(),
                            )
                        })?;
                        let su = frame.super_obj.ok_or_else(|| {
                            RuntimeError::new(
                                self.get_current_span(),
                                "'super' used outside of object scope".to_string(),
                                self.current_chunk().source_id.to_string(),
                            )
                        })?;
                        (s, su)
                    };

                    if let Value::String(field_key) = field_name {
                        // Check the field cache (self = self_obj_key for SuperIndex)
                        let cache_key = (super_obj_key, field_key, self_obj_key);
                        if let Some(&cached) = self.field_cache.get(&cache_key) {
                            self.push(cached)?;
                            self.advance_pc();
                            continue;
                        }

                        let resolution = self.get_object_field_resolution(super_obj_key, field_key);

                        if let Some((field, defining_node)) = resolution {
                            match field.value {
                                Value::Closure(closure_idx) => {
                                    self.advance_pc();
                                    self.push(Value::Closure(closure_idx))?;
                                    self.push(Value::Object(self_obj_key))?; // ORIGINAL self

                                    let new_super =
                                        self.memory_manager.load_object(defining_node).base_object;
                                    let super_val =
                                        new_super.map(Value::Object).unwrap_or(Value::Null);
                                    self.push(super_val)?;

                                    // Set field name for LoadFieldName opcode
                                    self.pending_field_name = Some(field_key);
                                    // Set cache target so Return can cache the result
                                    self.pending_cache_target = Some(cache_key);
                                    self.call_closure(
                                        closure_idx,
                                        2,
                                        Some(self_obj_key),
                                        new_super,
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("Super index must be a string, got {:?}", field_name),
                            self.current_chunk().source_id.to_string(),
                        ));
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("Invalid constant index: {}", index),
                            chunk.source_id.to_string(),
                        ));
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!(
                                "Invalid stack slot {} (stack size: {})",
                                stack_slot,
                                self.stack.len()
                            ),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }

                    // Copy value from slot to top of stack
                    let value = self.stack[stack_slot];
                    let result = self.force_value(value)?;
                    self.stack[stack_slot] = result;
                    self.push(result)?;
                }

                // Binary arithmetic operations
                Opcode::Add => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;

                    // Check for different addition types
                    // String concat is checked first (except for obj+obj and arr+arr)
                    // because Jsonnet implicitly stringifies when either operand is a string.
                    match (&a, &b) {
                        // Object merging (according to Jsonnet spec)
                        (Value::Object(left_key), Value::Object(right_key)) => {
                            let left_key = *left_key;
                            let right_key = *right_key;
                            let merged_key = self.merge_objects(left_key, right_key)?;
                            self.push(Value::Object(merged_key))?;
                            if self.memory_manager.should_collect() {
                                self.run_garbage_collection();
                            }
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
                        // String concatenation if either operand is a string
                        // Must be before object/array error cases since Jsonnet
                        // implicitly stringifies when either operand is a string.
                        (Value::String(_), _) | (_, Value::String(_)) => {
                            let a_str = self.value_to_string_for_concat(&a)?;
                            let b_str = self.value_to_string_for_concat(&b)?;
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
                        (Value::Array(_), _) | (_, Value::Array(_)) => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Must concatenate arrays with other arrays".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                        (Value::Object(_), _) | (_, Value::Object(_)) => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Must concatenate objects with other objects".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Division by zero".to_string(),
                            self.current_chunk().source_id.to_string(),
                        ));
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
                        // Force all thunks in arguments before passing to format
                        if let Value::Object(obj_idx) = b {
                            self.force_all_object_fields(obj_idx)?;
                        }
                        if let Value::Array(arr_idx) = b {
                            self.force_all_array_elements(arr_idx)?;
                        }
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Modulo by zero".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                    let result = self.compare_values(a, b)? == std::cmp::Ordering::Less;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Le => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.compare_values(a, b)? != std::cmp::Ordering::Greater;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Gt => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.compare_values(a, b)? == std::cmp::Ordering::Greater;
                    self.push(Value::Boolean(result))?;
                    self.advance_pc();
                }

                Opcode::Ge => {
                    let b = self.pop_forced()?;
                    let a = self.pop_forced()?;
                    let result = self.compare_values(a, b)? != std::cmp::Ordering::Less;
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
                                Value::Function(_)
                                | Value::NativeFunction(_)
                                | Value::NativeThunk(_) => "{function}".to_string(),
                                Value::Closure(_) => "{closure}".to_string(),
                                Value::Import(_) => "{import}".to_string(),
                                Value::Binary(_) => "{binary}".to_string(),
                                Value::Uninitialized => "{uninitialized}".to_string(),
                            };
                            let b_str = match &b {
                                Value::String(s) => self.memory_manager.load_string(*s).to_owned(),
                                Value::Number(n) => n.to_string(),
                                Value::Boolean(b) => b.to_string(),
                                Value::Null => "null".to_string(),
                                Value::Object(_) => "{object}".to_string(),
                                Value::Array(_) => "{array}".to_string(),
                                Value::Function(_)
                                | Value::NativeFunction(_)
                                | Value::NativeThunk(_) => "{function}".to_string(),
                                Value::Closure(_) => "{closure}".to_string(),
                                Value::Import(_) => "{import}".to_string(),
                                Value::Binary(_) => "{binary}".to_string(),
                                Value::Uninitialized => "{uninitialized}".to_string(),
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Invalid shift count".to_string(),
                            self.current_chunk().source_id.to_string(),
                        ));
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!(
                                "StoreVar: slot {} is out of range (stack size: {})",
                                slot,
                                self.stack.len()
                            ),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }

                    self.stack[abs_slot] = value;
                    // Note: read_u16_operand() already advanced PC by 3 (opcode + u16)
                    // No need to call advance_pc() here
                }

                Opcode::BindDefault => {
                    // Pops top of stack. If the value is NOT Uninitialized,
                    // skip forward by u16 offset (argument was provided, no default needed).
                    // If Uninitialized, fall through to the default initialization code.
                    let jump_offset = self.read_u16_operand()? as usize;
                    let value = self.pop()?;
                    if !matches!(value, Value::Uninitialized) {
                        // Argument was provided — push it back and skip default code
                        // Actually we don't need to push it back since StoreVar at the end
                        // of the default code would overwrite anyway. But we need to keep
                        // the slot as-is (it already has the value from the caller).
                        self.current_frame_mut().ip += jump_offset;
                    }
                    // If Uninitialized, fall through to compile and store the default thunk
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
                                    ObjectField::new(value, FieldVisibility::Visible),
                                );
                            }
                            Value::Null => {
                                // Null keys are omitted as per Jsonnet spec
                            }
                            _ => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("Object key must be a string or null, got {:?}", key),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                            // Attach the assertion to the object for deferred checking.
                            // Assertions are checked when the object is manifested or a field is accessed,
                            // not at construction time, because they may reference self fields
                            // that haven't been added yet.
                            if let Some(obj) = self.memory_manager.get_object_mut(obj_idx) {
                                obj.assertions.push(closure_idx);
                            }

                            // Push object back onto stack
                            self.push(Value::Object(obj_idx))?;
                        }
                        (c, o) => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!(
                                    "Invalid operands for Opcode::Assert: expected closure and object, got {:?} and {:?}",
                                    c, o
                                ),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                                    properties.insert(key_str, ObjectField::new(value, visibility));
                                }
                                Value::Null => {
                                    // Null keys are omitted
                                }
                                _ => {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Object key must be a string or null, got {:?}",
                                            key
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Expected object for ObjectInsert, got {:?}", object_val),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                            // Check the field cache first (self = object_key for ObjectIndex)
                            let cache_key = (object_key, field_key, object_key);
                            if let Some(&cached) = self.field_cache.get(&cache_key) {
                                self.push(cached)?;
                                self.advance_pc();
                                continue;
                            }

                            let resolution =
                                self.get_object_field_resolution(object_key, field_key);

                            if let Some((field, defining_node)) = resolution {
                                match field.value {
                                    Value::Closure(closure_idx) => {
                                        // It's a thunk! We need to call it with (self, super)
                                        self.advance_pc(); // Advance past ObjectIndex before calling
                                        self.push(Value::Closure(closure_idx))?;
                                        self.push(Value::Object(object_key))?; // self

                                        let new_super = self
                                            .memory_manager
                                            .load_object(defining_node)
                                            .base_object;
                                        let super_val =
                                            new_super.map(Value::Object).unwrap_or(Value::Null);
                                        self.push(super_val)?; // super

                                        // Set field name for LoadFieldName opcode
                                        self.pending_field_name = Some(field_key);
                                        // Set cache target so Return can cache the result
                                        self.pending_cache_target = Some(cache_key);
                                        self.call_closure(
                                            closure_idx,
                                            2,
                                            Some(object_key),
                                            new_super,
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Object index must be a string, got {:?}", field_name),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    } else {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("Cannot index into non-object value: {:?}", object_value),
                            self.current_chunk().source_id.to_string(),
                        ));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Array index cannot be negative, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                // Check for non-integer index
                                if index_num.fract() != 0.0 {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Array index must be an integer, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                let index = index_num as usize;
                                let array = self.memory_manager.load_array(array_key);

                                // Bounds check
                                if index >= array.len() {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Array index {} out of bounds (length: {})",
                                            index,
                                            array.len()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                let element = array.elements[index];
                                // Force thunks lazily - only evaluate the accessed element
                                let forced = self.force_array_element(array_key, index, element)?;
                                self.push(forced)?;
                            } else {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("Array index must be a number, got {:?}", index_value),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        }
                        Value::Binary(binary_key) => {
                            // Binary indexing with number
                            if let Value::Number(index_num) = index_value {
                                // Check for negative index
                                if index_num < 0.0 {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Binary index cannot be negative, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                // Check for non-integer index
                                if index_num.fract() != 0.0 {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Binary index must be an integer, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                let index = index_num as usize;
                                let binary = self.memory_manager.load_binary(binary_key);

                                // Bounds check
                                if index >= binary.data.len() {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "Binary index {} out of bounds (length: {})",
                                            index,
                                            binary.data.len()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }

                                self.push(Value::Number(binary.data[index] as f64))?;
                            } else {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("Binary index must be a number, got {:?}", index_value),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        }
                        Value::Object(object_key) => {
                            // Check deferred assertions before field access
                            self.check_object_assertions(object_key)?;
                            // Object indexing with string
                            if let Value::String(field_key) = index_value {
                                let resolution =
                                    self.get_object_field_resolution(object_key, field_key);

                                if let Some((field, defining_node)) = resolution {
                                    match field.value {
                                        Value::Closure(closure_idx) => {
                                            self.advance_pc();
                                            self.push(Value::Closure(closure_idx))?;
                                            self.push(Value::Object(object_key))?; // self
                                            let new_super = self
                                                .memory_manager
                                                .load_object(defining_node)
                                                .base_object;
                                            let super_val =
                                                new_super.map(Value::Object).unwrap_or(Value::Null);
                                            self.push(super_val)?;
                                            self.call_closure(
                                                closure_idx,
                                                2,
                                                Some(object_key),
                                                new_super,
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("Object index must be a string, got {:?}", index_value),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        }
                        Value::String(string_key) => {
                            // String indexing with number - returns single character string
                            if let Value::Number(index_num) = index_value {
                                if index_num < 0.0 {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "String index cannot be negative, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                                if index_num.fract() != 0.0 {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "String index must be an integer, got {}",
                                            index_num
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                                let index = index_num as usize;
                                let s = self.memory_manager.load_string(string_key).to_string();
                                let chars: Vec<char> = s.chars().collect();
                                if index >= chars.len() {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "String index {} out of bounds (length: {})",
                                            index,
                                            chars.len()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                                let ch = chars[index].to_string();
                                let str_alloc = self.memory_manager.allocate_string(&ch);
                                let str_idx = str_alloc.index;
                                self.push(Value::String(str_idx))?;
                            } else {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("String index must be a number, got {:?}", index_value),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Cannot index into value: {:?}", container_value),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Cannot get length of non-array value: {:?}", array_value),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Cannot append to non-array value: {:?}", array_value),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    }

                    self.advance_pc();
                }

                Opcode::ArrayAppendInPlace => {
                    // Pops TOS (value to append) and pushes it directly into the
                    // array stored at `slot`, mutating it in-place with no allocation.
                    // Safe because the comprehension result array is always private
                    // (created as __comp_result and never shared with user code).
                    let slot = self.read_u16_operand()? as usize;
                    let value_to_append = self.pop()?;

                    let frame_base = self.current_frame().stack_base;
                    let abs_slot = frame_base + slot;

                    let array_key = match self.stack[abs_slot] {
                        Value::Array(key) => key,
                        ref v => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!(
                                    "ArrayAppendInPlace: slot {} is not an array: {:?}",
                                    slot, v
                                ),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };

                    self.memory_manager
                        .load_array_mut(array_key)
                        .elements
                        .push(value_to_append);
                    // read_u16_operand() already advanced PC by 3 (opcode + u16).
                    // result_slot still holds the same array key — no StoreVar needed.
                }

                Opcode::ObjectMerge => {
                    let right_value = self.pop_forced()?; // Right-hand side object
                    let left_value = self.pop_forced()?; // Left-hand side object

                    // Ensure both values are objects
                    if let (Value::Object(left_key), Value::Object(right_key)) =
                        (left_value, right_value)
                    {
                        // Lazy merge: right properties with base_object pointing to left
                        let (right_props_raw, right_assertions): (Vec<_>, Vec<_>) = {
                            let right_object = self.memory_manager.load_object(right_key);
                            (
                                right_object
                                    .properties
                                    .iter()
                                    .map(|(k, f)| (*k, f.value, f.visibility))
                                    .collect(),
                                right_object.assertions.clone(),
                            )
                        };
                        let mut merged_properties = std::collections::HashMap::new();
                        for (key, value, vis_raw) in right_props_raw {
                            // Visibility inheritance: if right is ':', inherit from left chain if present
                            let visibility = if vis_raw == FieldVisibility::Visible {
                                self.get_object_field_resolution(left_key, key)
                                    .map(|(f, _)| f.visibility)
                                    .unwrap_or(FieldVisibility::Visible)
                            } else {
                                vis_raw
                            };
                            merged_properties.insert(key, ObjectField::new(value, visibility));
                        }

                        let merged_allocation = self.memory_manager.allocate_object_full_with_base(
                            Some(left_key),
                            merged_properties,
                            right_assertions,
                        );
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Object merge requires two objects".to_string(),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }
                    self.advance_pc();
                }

                Opcode::StdCall => {
                    let frame = self.current_frame();
                    let chunk = self.current_chunk();

                    if frame.ip + 3 >= chunk.count() {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Invalid bytecode - missing StdCall operands".to_string(),
                            chunk.source_id.to_string(),
                        ));
                    }

                    let func_id_val = chunk.read_u16(frame.ip + 1).unwrap();
                    let arg_count = chunk.read_u8(frame.ip + 3).unwrap() as usize;
                    self.current_frame_mut().ip += 4; // opcode + u16 + u8

                    let func_id = chunk::NativeFuncId::from_u16(func_id_val).ok_or_else(|| {
                        RuntimeError::new(
                            self.get_current_span(),
                            format!("Invalid native function ID: {}", func_id_val),
                            self.current_chunk().source_id.to_string(),
                        )
                    })?;

                    // Extract arguments from stack
                    let mut args = self.stack[self.stack.len() - arg_count..].to_vec();
                    // Pop arguments and the native function value itself
                    for _ in 0..=arg_count {
                        self.pop()?;
                    }

                    // Force all arguments
                    for arg in args.iter_mut() {
                        *arg = self.force_value(*arg)?;
                    }

                    // For specific native functions, recursively force container elements
                    if matches!(
                        func_id,
                        chunk::NativeFuncId::Join | chunk::NativeFuncId::Format
                    ) {
                        // Root ALL args before forcing any container elements.
                        // force_all_array_elements only roots the specific array being
                        // processed; without this, other args can be freed by GC when a
                        // thunk element allocates (e.g. a nested array literal creates
                        // closures, triggering a collection under stress_gc).
                        self.memory_manager.external_roots.push(args.clone());
                        for arg in args.iter() {
                            match arg {
                                Value::Array(a) => self.force_all_array_elements(*a)?,
                                Value::Object(o) => self.force_all_object_fields(*o)?,
                                _ => {}
                            }
                        }
                        self.memory_manager.external_roots.pop();
                    }

                    // Handle std.get: field value may be a thunk closure
                    if func_id == chunk::NativeFuncId::Get {
                        let span = self.get_current_span();
                        let source_id = self.current_chunk().source_id.to_string();
                        let o_val = args[0];
                        let f_val = args[1];
                        let default_val = args.get(2).copied().unwrap_or(Value::Null);
                        let inc_hidden_val = args.get(3).copied().unwrap_or(Value::Null);

                        let o_idx = match o_val {
                            Value::Object(o) => o,
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "std.get() first argument must be an object".to_string(),
                                    source_id,
                                ));
                            }
                        };
                        let field_name = match f_val {
                            Value::String(s_idx) => {
                                self.memory_manager.load_string(s_idx).to_string()
                            }
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "std.get() second argument must be a string".to_string(),
                                    source_id,
                                ));
                            }
                        };
                        let inc_hidden = match inc_hidden_val {
                            Value::Boolean(b) => b,
                            Value::Null => true,
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "std.get() fourth argument must be a boolean or null"
                                        .to_string(),
                                    source_id,
                                ));
                            }
                        };

                        // Walk the chain to find the field (inc_hidden controls visibility filter)
                        let found = self.enumerate_object_fields(o_idx).into_iter().find(
                            |(k, _, _, vis)| {
                                self.memory_manager.load_string(*k) == field_name.as_str()
                                    && (inc_hidden || *vis != FieldVisibility::Hidden)
                            },
                        );

                        let result = match found {
                            Some((_, val, super_obj, _)) => match val {
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
                            },
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
                            // Collect all (key, raw_value, super_obj) pairs from chain
                            let pairs = self.enumerate_object_fields(o_idx);
                            // Evaluate each field
                            let mut evaluated: Vec<(StringIndex, Value)> =
                                Vec::with_capacity(pairs.len());
                            for (k, v, super_obj, _vis) in pairs {
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
                                    memory_manager::ObjectField::new(v, FieldVisibility::Visible),
                                );
                            }
                            let new_obj_alloc = self
                                .memory_manager
                                .allocate_object_with_properties(properties);
                            let new_obj_val = Value::Object(new_obj_alloc.index);
                            // Now call format with the new object
                            let new_args = vec![args[0], new_obj_val];
                            let result =
                                self.call_native_checked(func_id, &new_args, span, source_id)?;
                            self.push(result)?;
                            continue;
                        }
                    }

                    if func_id == chunk::NativeFuncId::MakeArray {
                        let sz_val = args[0];
                        let func_val = args[1];

                        let sz = match sz_val {
                            Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as usize,
                            _ => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.makeArray expected positive integer for size, got {:?}",
                                        sz_val
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };

                        let mut elements = Vec::with_capacity(sz);
                        let mut should_gc = false;
                        for i in 0..sz {
                            let thunk_alloc = self
                                .memory_manager
                                .allocate_native_thunk(func_val, Value::Number(i as f64));
                            should_gc |= thunk_alloc.should_garbage_collect;
                            elements.push(Value::NativeThunk(thunk_alloc.index));
                        }
                        self.memory_manager
                            .push_external_roots(elements.clone(), Vec::new());
                        let array_alloc = self.memory_manager.allocate_array(elements);
                        self.memory_manager.pop_external_roots();
                        if should_gc || array_alloc.should_garbage_collect {
                            let result_val = Value::Array(array_alloc.index);
                            self.memory_manager
                                .push_external_roots(vec![result_val], Vec::new());
                            self.run_garbage_collection();
                            self.memory_manager.pop_external_roots();
                        }
                        self.push(Value::Array(array_alloc.index))?;
                        continue;
                    }

                    // Handle std.sort with keyF
                    if func_id == chunk::NativeFuncId::Sort && args.len() == 2 {
                        let arr_val = args[0];
                        let key_f = args[1];
                        let arr_idx = match arr_val {
                            Value::Array(a) => a,
                            _ => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.sort() expected array as first argument".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.{} expected array, got {}",
                                        func_id.name(),
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };

                        // Root args before forcing array elements (which may trigger GC)
                        let mut roots = vec![arr_val];
                        if let Some(kf) = key_f {
                            roots.push(kf);
                        }
                        if let Some(oe) = on_empty {
                            roots.push(oe);
                        }
                        self.memory_manager.external_roots.push(roots);
                        self.force_all_array_elements(arr_idx)?;
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();

                        if elements.is_empty() {
                            self.memory_manager.external_roots.pop();
                            match on_empty {
                                Some(v) => {
                                    self.push(v)?;
                                    continue;
                                }
                                None => {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!("std.{}: empty array", func_id.name()),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                            self.memory_manager.external_roots.pop(); // pop args roots
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.uniq() expected array as first argument".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        // Root keyF before force_all_array_elements which may trigger GC
                        self.memory_manager
                            .external_roots
                            .push(vec![key_f, arr_val]);
                        self.force_all_array_elements(arr_idx)?;
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
                        self.memory_manager.external_roots.pop(); // pop keyF/arrVal roots
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
                            // Collect all (key, raw_value, super_obj, visibility) tuples from chain
                            let field_data = self.enumerate_object_fields(o_idx);
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
                                properties.insert(k, memory_manager::ObjectField::new(v, vis));
                            }
                            let new_obj_alloc = self
                                .memory_manager
                                .allocate_object_with_properties(properties);
                            let new_obj_val = Value::Object(new_obj_alloc.index);
                            let new_args = vec![new_obj_val];
                            let result =
                                self.call_native_checked(func_id, &new_args, span, source_id)?;
                            self.push(result)?;
                            continue;
                        }
                    }

                    // Handle std.toString via compact single-line format
                    if func_id == chunk::NativeFuncId::ToString {
                        if matches!(args[0], Value::String(_)) {
                            self.push(args[0])?;
                        } else {
                            let s = self.value_to_string_for_concat(&args[0])?;
                            let alloc = self.memory_manager.allocate_string(&s);
                            self.push(Value::String(alloc.index))?;
                        }
                        continue;
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
                                        return Err(RuntimeError::new(
                                            span,
                                            "std.manifestJsonEx: indent must be a string"
                                                .to_string(),
                                            source_id,
                                        ));
                                    }
                                };
                                let n = if args.len() > 2 {
                                    match args[2] {
                                        Value::String(s_idx) => {
                                            self.memory_manager.load_string(s_idx).to_string()
                                        }
                                        _ => {
                                            return Err(RuntimeError::new(
                                                span,
                                                "std.manifestJsonEx: newline must be a string"
                                                    .to_string(),
                                                source_id,
                                            ));
                                        }
                                    }
                                } else {
                                    "\n".to_string()
                                };
                                let k =
                                    if args.len() > 3 {
                                        match args[3] {
                                            Value::String(s_idx) => {
                                                self.memory_manager.load_string(s_idx).to_string()
                                            }
                                            _ => return Err(RuntimeError::new(
                                                span,
                                                "std.manifestJsonEx: key_val_sep must be a string"
                                                    .to_string(),
                                                source_id,
                                            )),
                                        }
                                    } else {
                                        ": ".to_string()
                                    };
                                (i, n, k)
                            }
                            _ => unreachable!(),
                        };
                        let value = args[0];
                        let mut json = self.manifest_json_value(
                            value,
                            &indent,
                            &newline,
                            &kvs,
                            0,
                            span.clone(),
                            &source_id,
                        )?;
                        // manifestJson uses "[ ]" and "{ }" for empty collections
                        if func_id == chunk::NativeFuncId::ManifestJson {
                            json = Self::fix_manifest_json_empties(&json);
                        }
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.map: second argument must be an array".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results = Vec::with_capacity(elements.len());
                        for &elem in &elements {
                            // Only root things NOT already on stack or in args (if args were rooted).
                            // Since args is not rooted in StdCall yet, we must root func_val and elements.
                            let mut roots = Vec::new();
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(func_val);
                            roots.push(elem);

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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.filter: second argument must be an array".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                        let mut results = Vec::new();
                        for &elem in &elements {
                            let mut roots = Vec::new();
                            roots.extend_from_slice(&elements);
                            roots.extend_from_slice(&results);
                            roots.push(func_val);
                            roots.push(elem);
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.filter: function must return boolean, got {:?}",
                                            other
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.foldl: second argument must be an array".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                            return Err(RuntimeError::new(self.get_current_span(), "std.flatMap: function must return array for array input"
                                                        .to_string(), self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string()));
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
                                            return Err(RuntimeError::new(self.get_current_span(), "std.flatMap: function must return string for string input"
                                                        .to_string(), self
                                                    .current_chunk()
                                                    .source_id
                                                    .to_string()));
                                        }
                                    }
                                }
                                let alloc = self.memory_manager.allocate_string(&out);
                                self.push(Value::String(alloc.index))?;
                            }
                            _ => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.flatMap: second argument must be array or string"
                                        .to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.mapWithIndex: second argument must be an array"
                                        .to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.foldr: second argument must be an array".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.mapWithKey: second argument must be an object".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        // Collect visible fields across the full base_object chain.
                        let field_data: Vec<(StringIndex, Value, FieldVisibility)> = self
                            .enumerate_object_fields(o_idx)
                            .into_iter()
                            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                            .map(|(k, v, _, vis)| (k, v, vis))
                            .collect();
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
                            for (k, f) in new_properties.iter() {
                                roots.push(Value::String(*k));
                                roots.push(f.value);
                            }
                            roots.push(Value::Object(o_idx));
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
                            new_properties.insert(k_idx, ObjectField::new(result?, vis));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    "std.filterMap: third argument must be an array".to_string(),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        "std.filterMap: filter function must return a boolean"
                                            .to_string(),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                return Err(RuntimeError::new(
                                    span,
                                    format!(
                                        "std.extVar: argument must be a string, got {}",
                                        other.type_name()
                                    ),
                                    source_id,
                                ));
                            }
                        };
                        match self.ext_vars.get(&key).copied() {
                            Some(val) => {
                                self.push(val)?;
                                continue;
                            }
                            None => {
                                return Err(RuntimeError::new(
                                    span,
                                    format!("Undefined external variable: '{}'", key),
                                    source_id,
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setInter: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setInter: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setDiff: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setDiff: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setMember: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setUnion: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let b_idx = match b_val {
                            Value::Array(i) => i,
                            other => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("setUnion: expected array, got {}", other.type_name()),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    span,
                                    "std.parseJson: argument must be a string".to_string(),
                                    source_id,
                                ));
                            }
                        };
                        let s = self.memory_manager.load_string(s_idx).to_string();
                        let result = self.parse_json_value(&s, span.clone(), &source_id)?;
                        self.push(result)?;
                        continue;
                    }

                    if matches!(
                        func_id,
                        chunk::NativeFuncId::MergePatch
                            | chunk::NativeFuncId::Prune
                            | chunk::NativeFuncId::Uniq
                            | chunk::NativeFuncId::Sort
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
                            chunk::NativeFuncId::Sort => {
                                let result = self.sort_value(args[0], args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::Set => {
                                let arr_val = args[0];
                                if let Value::Array(a) = arr_val {
                                    self.force_all_array_elements(a)?;
                                }
                                let sorted = self.sort_value(arr_val, None)?;
                                let result = self.uniq_value(sorted, args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            chunk::NativeFuncId::SetUnion => {
                                let a_val = args[0];
                                let b_val = args[1];
                                let a_idx = match a_val {
                                    Value::Array(i) => {
                                        self.force_all_array_elements(i)?;
                                        i
                                    }
                                    _ => {
                                        return Err(RuntimeError::new(
                                            span,
                                            "std.setUnion: first argument must be an array"
                                                .to_string(),
                                            source_id,
                                        ));
                                    }
                                };
                                let b_idx = match b_val {
                                    Value::Array(i) => {
                                        self.force_all_array_elements(i)?;
                                        i
                                    }
                                    _ => {
                                        return Err(RuntimeError::new(
                                            span,
                                            "std.setUnion: second argument must be an array"
                                                .to_string(),
                                            source_id,
                                        ));
                                    }
                                };
                                let mut combined =
                                    self.memory_manager.load_array(a_idx).elements.clone();
                                combined.extend_from_slice(
                                    &self.memory_manager.load_array(b_idx).elements.clone(),
                                );
                                let alloc = self.memory_manager.allocate_array(combined);
                                let sorted = self.sort_value(Value::Array(alloc.index), None)?;
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
                        let indent_array_in_object =
                            match args.get(1).copied().unwrap_or(Value::Null) {
                                Value::Boolean(b) => b,
                                Value::Null => false,
                                _ => {
                                    return Err(RuntimeError::new(
                                        span,
                                        "manifestYamlDoc: indent_array_in_object must be bool"
                                            .to_string(),
                                        source_id,
                                    ));
                                }
                            };
                        let quote_keys = match args.get(2).copied().unwrap_or(Value::Null) {
                            Value::Boolean(b) => b,
                            Value::Null => true,
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "manifestYamlDoc: quote_keys must be bool".to_string(),
                                    source_id,
                                ));
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
                        let indent_array_in_object =
                            match args.get(1).copied().unwrap_or(Value::Null) {
                                Value::Boolean(b) => b,
                                Value::Null => false,
                                _ => {
                                    return Err(RuntimeError::new(
                                        span,
                                        "manifestYamlStream: indent_array_in_object must be bool"
                                            .to_string(),
                                        source_id,
                                    ));
                                }
                            };
                        let c_document_end = match args.get(2).copied().unwrap_or(Value::Null) {
                            Value::Boolean(b) => b,
                            Value::Null => true,
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "manifestYamlStream: c_document_end must be bool".to_string(),
                                    source_id,
                                ));
                            }
                        };
                        let quote_keys = match args.get(3).copied().unwrap_or(Value::Null) {
                            Value::Boolean(b) => b,
                            Value::Null => true,
                            _ => {
                                return Err(RuntimeError::new(
                                    span,
                                    "manifestYamlStream: quote_keys must be bool".to_string(),
                                    source_id,
                                ));
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
                                return Err(RuntimeError::new(
                                    span,
                                    "parseYaml: argument must be a string".to_string(),
                                    source_id,
                                ));
                            }
                        };
                        let result = self.parse_yaml_multi_doc(&s, span, source_id)?;
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
                                return Err(RuntimeError::new(
                                    span,
                                    "manifestTomlEx: indent must be a string".to_string(),
                                    source_id,
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.groupBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.groupBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                memory_manager::ObjectField::new(
                                    Value::Array(arr_alloc.index),
                                    FieldVisibility::Visible,
                                ),
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.sortBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.countBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.countBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                memory_manager::ObjectField::new(
                                    Value::Number(count as f64),
                                    FieldVisibility::Visible,
                                ),
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.uniqBy: expected array, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.uniqBy: keyF must return string, got {}",
                                            other.type_name()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.{}: expected array, got {}",
                                        func_id.name(),
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        // Root keyF before forcing array elements (which may trigger GC)
                        self.memory_manager
                            .external_roots
                            .push(vec![key_f, arr_val]);
                        self.force_all_array_elements(arr_idx)?;
                        let elements: Vec<Value> =
                            self.memory_manager.load_array(arr_idx).elements.clone();
                        if elements.is_empty() {
                            self.memory_manager.external_roots.pop();
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("std.{}: array must not be empty", func_id.name()),
                                self.current_chunk().source_id.to_string(),
                            ));
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
                        self.memory_manager.external_roots.pop(); // pop keyF/arrVal roots
                        self.push(best_elem)?;
                        continue;
                    }

                    // Handle std.toPairs
                    if func_id == chunk::NativeFuncId::ToPairs {
                        let obj_val = args[0];
                        let o_idx = match obj_val {
                            Value::Object(i) => i,
                            other => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.toPairs: expected object, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value)> = self
                            .enumerate_object_fields(o_idx)
                            .into_iter()
                            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                            .map(|(k, v, _, _)| (k, v))
                            .collect();
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.mapKeys: expected object, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = self
                            .enumerate_object_fields(o_idx)
                            .into_iter()
                            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                            .map(|(k, v, _, vis)| (k, v, vis))
                            .collect();
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
                            for (k, f) in new_properties.iter() {
                                roots.push(Value::String(*k));
                                roots.push(f.value);
                            }
                            roots.push(Value::Object(o_idx));
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
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.mapKeys: func must return string, got {}",
                                            other.type_name()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            };
                            let new_k_alloc = self.memory_manager.allocate_string(&new_key_str);
                            new_properties.insert(
                                new_k_alloc.index,
                                memory_manager::ObjectField::new(evaled_val, vis),
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.filterObject: expected object, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let field_data: Vec<(chunk::StringIndex, Value, FieldVisibility)> = self
                            .enumerate_object_fields(o_idx)
                            .into_iter()
                            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                            .map(|(k, v, _, vis)| (k, v, vis))
                            .collect();
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
                            let mut roots = Vec::new();
                            roots.push(func_val);
                            roots.push(evaled_val);
                            roots.push(key_val);
                            for (k, f) in kept_properties.iter() {
                                roots.push(Value::String(*k));
                                roots.push(f.value);
                            }
                            roots.push(Value::Object(o_idx));
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
                                        memory_manager::ObjectField::new(evaled_val, vis),
                                    );
                                }
                                Value::Boolean(false) => {}
                                other => {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.filterObject: func must return bool, got {}",
                                            other.type_name()
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "std.objectFlatten: sep must be string, got {}",
                                        other.type_name()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        };
                        let mut flat_fields: Vec<(String, Value)> = Vec::new();

                        // Root arguments during recursive flattening
                        self.memory_manager
                            .push_external_roots(args.clone(), Vec::new());
                        let flatten_res = self.flatten_object_recursive(
                            obj_val,
                            &sep,
                            String::new(),
                            &mut flat_fields,
                        );
                        self.memory_manager.pop_external_roots();
                        flatten_res?;

                        let mut properties: std::collections::HashMap<
                            chunk::StringIndex,
                            memory_manager::ObjectField,
                        > = std::collections::HashMap::new();

                        // Root the values in flat_fields while building the object
                        let flat_vals: Vec<Value> = flat_fields.iter().map(|(_, v)| *v).collect();
                        self.memory_manager
                            .push_external_roots(flat_vals, Vec::new());

                        for (k_str, v) in flat_fields {
                            let k_alloc = self.memory_manager.allocate_string(&k_str);
                            properties.insert(
                                k_alloc.index,
                                memory_manager::ObjectField::new(v, FieldVisibility::Visible),
                            );
                        }
                        let obj_alloc = self
                            .memory_manager
                            .allocate_object_with_properties(properties);

                        self.memory_manager.pop_external_roots();

                        self.push(Value::Object(obj_alloc.index))?;

                        if obj_alloc.should_garbage_collect {
                            self.run_garbage_collection();
                        }
                        continue;
                    }

                    // Call native function
                    let span = self.get_current_span();
                    let source_id = self.current_chunk().source_id.to_string();
                    let result = self.call_native_checked(func_id, &args, span, source_id)?;

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
                    return Err(RuntimeError::new(
                        self.get_current_span(),
                        error_message,
                        self.current_chunk().source_id.to_string(),
                    ));
                }

                Opcode::Import => {
                    // Read constant index, which points to a string constant (the path)
                    let const_idx = self.read_u16_operand()?;

                    let path_str_idx = match self.current_chunk().constants.get(const_idx as usize)
                    {
                        Some(Value::String(idx)) => *idx,
                        _ => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Import operand must be a string constant".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    let target_path_str = self.resolve_import_path(&path_str);

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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "ImportStr operand must be a string constant".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    let target_path_str = self.resolve_import_path(&path_str);

                    // Read the file content
                    let content = std::fs::read_to_string(&target_path_str).map_err(|e| {
                        RuntimeError::new(
                            self.get_current_span(),
                            format!("Failed to read file '{}': {}", target_path_str, e),
                            self.current_chunk().source_id.to_string(),
                        )
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "ImportBin operand must be a string constant".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    };

                    let path_str = self.memory_manager.load_string(path_str_idx).to_string();

                    let target_path_str = self.resolve_import_path(&path_str);

                    // Read the file content as binary
                    let content = std::fs::read(&target_path_str).map_err(|e| {
                        RuntimeError::new(
                            self.get_current_span(),
                            format!("Failed to read file '{}': {}", target_path_str, e),
                            self.current_chunk().source_id.to_string(),
                        )
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Invalid bytecode - missing Call operands".to_string(),
                            chunk.source_id.to_string(),
                        ));
                    }

                    let positional_count = chunk.code[frame.ip + 1] as usize;
                    let named_count = chunk.code[frame.ip + 2] as usize;
                    self.current_frame_mut().ip += 3; // opcode + 2 bytes

                    // Total stack items for args: positional + named*2 (name+value pairs)
                    let total_stack_items = positional_count + named_count * 2;

                    // Get callee from stack
                    let callee_position = self.stack.len() - total_stack_items - 1;

                    let mut callee = self.stack[callee_position];
                    callee = self.force_value(callee)?;
                    self.stack[callee_position] = callee;

                    if named_count > 0 {
                        if let Value::Closure(closure_index) = callee {
                            // Resolve named arguments using function's param_names
                            let func_index =
                                self.memory_manager.load_closure(closure_index).function;
                            let function = self.memory_manager.load_function(func_index);
                            let arity = function.arity as usize;
                            let param_names = function.param_names.clone();

                            // Collect named args from stack (name, value pairs)
                            let mut named_args: Vec<(StringIndex, Value)> = Vec::new();
                            for _ in 0..named_count {
                                let value = self.pop()?;
                                let name_val = self.pop()?;
                                if let Value::String(name_idx) = name_val {
                                    named_args.push((name_idx, value));
                                } else {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        "Named argument name must be a string".to_string(),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }

                            // Collect positional args
                            let mut positional_args: Vec<Value> = Vec::new();
                            for _ in 0..positional_count {
                                positional_args.push(self.pop()?);
                            }
                            positional_args.reverse();

                            // Pop callee
                            self.pop()?;

                            // Build full argument array
                            let mut args = vec![Value::Uninitialized; arity];

                            // Fill positional args
                            for (i, val) in positional_args.into_iter().enumerate() {
                                if i < arity {
                                    args[i] = val;
                                }
                            }

                            // Fill named args by matching param names
                            for (name_idx, val) in named_args {
                                let pos = param_names.iter().position(|&pn| pn == name_idx);
                                if let Some(pos) = pos {
                                    if !matches!(args[pos], Value::Uninitialized) {
                                        let name_str =
                                            self.memory_manager.load_string(name_idx).to_string();
                                        return Err(RuntimeError::new(
                                            self.get_current_span(),
                                            format!(
                                                "Argument '{}' already provided positionally",
                                                name_str,
                                            ),
                                            self.current_chunk().source_id.to_string(),
                                        ));
                                    }
                                    args[pos] = val;
                                } else {
                                    let name_str =
                                        self.memory_manager.load_string(name_idx).to_string();
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!("Function has no parameter named '{}'", name_str,),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }

                            // Push callee and args back onto stack
                            self.push(Value::Closure(closure_index))?;
                            let arg_count = args.len();
                            for val in args {
                                self.push(val)?;
                            }
                            self.call_closure(closure_index, arg_count, None, None)?;
                        } else if let Value::NativeFunction(id) = callee {
                            // Resolve named arguments for native functions
                            let native_param_names = id.param_names();
                            let arity = id.arity() as usize;

                            let mut named_args: Vec<(StringIndex, Value)> = Vec::new();
                            for _ in 0..named_count {
                                let value = self.pop()?;
                                let name_val = self.pop()?;
                                if let Value::String(name_idx) = name_val {
                                    named_args.push((name_idx, value));
                                } else {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        "Named argument name must be a string".to_string(),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }

                            let mut positional_args: Vec<Value> = Vec::new();
                            for _ in 0..positional_count {
                                positional_args.push(self.pop()?);
                            }
                            positional_args.reverse();
                            self.pop()?; // pop callee

                            let mut args = vec![Value::Null; arity];
                            for (i, val) in positional_args.into_iter().enumerate() {
                                if i < arity {
                                    args[i] = val;
                                }
                            }
                            for (name_idx, val) in named_args {
                                let name_str =
                                    self.memory_manager.load_string(name_idx).to_string();
                                if let Some(pos) =
                                    native_param_names.iter().position(|&pn| pn == name_str)
                                {
                                    args[pos] = val;
                                } else {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        format!(
                                            "std.{} has no parameter named '{}'",
                                            id.name(),
                                            name_str,
                                        ),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }

                            // Push resolved args back onto stack as positional call
                            self.push(Value::NativeFunction(id))?;
                            let resolved_arg_count = args.len();
                            for val in args {
                                self.push(val)?;
                            }
                            // Fall through to the positional NativeFunction handler below
                            // by extracting args from stack
                            let args = self.stack[self.stack.len() - resolved_arg_count..].to_vec();
                            for _ in 0..=resolved_arg_count {
                                self.pop()?;
                            }

                            // Handle functions that require VM-level dispatch
                            if id == chunk::NativeFuncId::Sort {
                                let result = self.sort_value(args[0], args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            if id == chunk::NativeFuncId::Uniq {
                                let result = self.uniq_value(args[0], args.get(1).copied())?;
                                self.push(result)?;
                                continue;
                            }
                            // Delegate to the same handling as positional native calls
                            // (handles special cases like MakeArray, Map, etc.)
                            let span = self.get_current_span();
                            let source_id = self.current_chunk().source_id.to_string();
                            let result = self.call_native_checked(id, &args, span, source_id)?;
                            self.push(result)?;
                        } else {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Named arguments only supported for functions".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    } else {
                        let arg_count = positional_count;
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
                                        | chunk::NativeFuncId::Sort
                                        | chunk::NativeFuncId::Set
                                        | chunk::NativeFuncId::SetUnion
                                ) {
                                    let span = self.get_current_span();
                                    let source_id = self.current_chunk().source_id.to_string();

                                    match id {
                                        chunk::NativeFuncId::MergePatch => {
                                            let result =
                                                self.merge_patch_value(args[0], args[1])?;
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
                                        chunk::NativeFuncId::Sort => {
                                            let result =
                                                self.sort_value(args[0], args.get(1).copied())?;
                                            self.push(result)?;
                                            continue;
                                        }
                                        chunk::NativeFuncId::Set => {
                                            let arr_val = args[0];
                                            if let Value::Array(a) = arr_val {
                                                self.force_all_array_elements(a)?;
                                            }
                                            let sorted = self.sort_value(arr_val, None)?;
                                            let result =
                                                self.uniq_value(sorted, args.get(1).copied())?;
                                            self.push(result)?;
                                            continue;
                                        }
                                        chunk::NativeFuncId::SetUnion => {
                                            let a_val = args[0];
                                            let b_val = args[1];
                                            let a_idx = match a_val {
                                                Value::Array(i) => {
                                                    self.force_all_array_elements(i)?;
                                                    i
                                                }
                                                _ => return Err(RuntimeError::new(
                                                    span,
                                                    "std.setUnion: first argument must be an array"
                                                        .to_string(),
                                                    source_id,
                                                )),
                                            };
                                            let b_idx = match b_val {
                                            Value::Array(i) => {
                                                self.force_all_array_elements(i)?;
                                                i
                                            }
                                            _ => return Err(RuntimeError::new(span, "std.setUnion: second argument must be an array"
                                                        .to_string(), source_id)),
                                        };
                                            let mut combined = self
                                                .memory_manager
                                                .load_array(a_idx)
                                                .elements
                                                .clone();
                                            combined.extend_from_slice(
                                                &self
                                                    .memory_manager
                                                    .load_array(b_idx)
                                                    .elements
                                                    .clone(),
                                            );
                                            let alloc =
                                                self.memory_manager.allocate_array(combined);
                                            let sorted =
                                                self.sort_value(Value::Array(alloc.index), None)?;
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
                                    let result =
                                        self.manifest_python_vars(value, span, source_id)?;
                                    let idx = self.memory_manager.allocate_string(&result);
                                    self.push(Value::String(idx.index))?;
                                    continue;
                                }

                                // Handle manifestYamlDoc
                                if id == chunk::NativeFuncId::ManifestYamlDoc {
                                    let span = self.get_current_span();
                                    let source_id = self.current_chunk().source_id.to_string();
                                    let value = args[0];
                                    let indent_array_in_object = match args
                                        .get(1)
                                        .copied()
                                        .unwrap_or(Value::Null)
                                    {
                                        Value::Boolean(b) => b,
                                        Value::Null => false,
                                        _ => {
                                            return Err(RuntimeError::new(span, "manifestYamlDoc: indent_array_in_object must be bool"
                                                    .to_string(), source_id));
                                        }
                                    };
                                    let quote_keys =
                                        match args.get(2).copied().unwrap_or(Value::Null) {
                                            Value::Boolean(b) => b,
                                            Value::Null => true,
                                            _ => {
                                                return Err(RuntimeError::new(
                                                    span,
                                                    "manifestYamlDoc: quote_keys must be bool"
                                                        .to_string(),
                                                    source_id,
                                                ));
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
                                    let indent_array_in_object = match args
                                        .get(1)
                                        .copied()
                                        .unwrap_or(Value::Null)
                                    {
                                        Value::Boolean(b) => b,
                                        Value::Null => false,
                                        _ => {
                                            return Err(RuntimeError::new(span, "manifestYamlStream: indent_array_in_object must be bool".to_string(), source_id));
                                        }
                                    };
                                    let c_document_end =
                                        match args.get(2).copied().unwrap_or(Value::Null) {
                                            Value::Boolean(b) => b,
                                            Value::Null => true,
                                            _ => {
                                                return Err(RuntimeError::new(
                                                span,
                                                "manifestYamlStream: c_document_end must be bool"
                                                    .to_string(),
                                                source_id,
                                            ));
                                            }
                                        };
                                    let quote_keys =
                                        match args.get(3).copied().unwrap_or(Value::Null) {
                                            Value::Boolean(b) => b,
                                            Value::Null => true,
                                            _ => {
                                                return Err(RuntimeError::new(
                                                    span,
                                                    "manifestYamlStream: quote_keys must be bool"
                                                        .to_string(),
                                                    source_id,
                                                ));
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
                                            return Err(RuntimeError::new(
                                                span,
                                                "parseYaml: argument must be a string".to_string(),
                                                source_id,
                                            ));
                                        }
                                    };
                                    let result = self.parse_yaml_multi_doc(&s, span, source_id)?;
                                    self.push(result)?;
                                    continue;
                                }

                                // Handle manifestXmlJsonml
                                if id == chunk::NativeFuncId::ManifestXmlJsonml {
                                    let span = self.get_current_span();
                                    let source_id = self.current_chunk().source_id.to_string();
                                    let value = args[0];
                                    let result =
                                        self.manifest_xml_jsonml(value, span, source_id)?;
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
                                            return Err(RuntimeError::new(
                                                span,
                                                "manifestTomlEx: indent must be a string"
                                                    .to_string(),
                                                source_id,
                                            ));
                                        }
                                    };
                                    let result = self.manifest_toml_ex(
                                        value,
                                        &indent.clone(),
                                        span,
                                        source_id,
                                    )?;
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.{} expected array, got {}",
                                                    id.name(),
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };

                                    // Root args before forcing array elements (which may trigger GC)
                                    let mut roots = vec![arr_val];
                                    if let Some(kf) = key_f {
                                        roots.push(kf);
                                    }
                                    if let Some(oe) = on_empty {
                                        roots.push(oe);
                                    }
                                    self.memory_manager.external_roots.push(roots);
                                    self.force_all_array_elements(arr_idx)?;
                                    let elements: Vec<Value> =
                                        self.memory_manager.load_array(arr_idx).elements.clone();

                                    if elements.is_empty() {
                                        self.memory_manager.external_roots.pop();
                                        match on_empty {
                                            Some(v) => {
                                                self.push(v)?;
                                                continue;
                                            }
                                            None => {
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!("std.{}: empty array", id.name()),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
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
                                                upvalue =
                                                    self.memory_manager.load_upvalue(uv_idx).next;
                                            }
                                            self.memory_manager
                                                .push_external_roots(roots, open_upvalue_roots);
                                            let k = self
                                                .call_value_with_one_arg(key_f_val, elements[0]);
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
                                                    upvalue = self
                                                        .memory_manager
                                                        .load_upvalue(uv_idx)
                                                        .next;
                                                }
                                                self.memory_manager
                                                    .push_external_roots(roots, open_upvalue_roots);
                                                let k =
                                                    self.call_value_with_one_arg(key_f_val, elem);
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
                                        self.memory_manager.external_roots.pop(); // pop args roots
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
                                        self.memory_manager.external_roots.pop(); // pop args roots
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
                                            return Err(RuntimeError::new(
                                                span,
                                                format!(
                                                    "std.extVar: argument must be a string, got {}",
                                                    other.type_name()
                                                ),
                                                source_id,
                                            ));
                                        }
                                    };
                                    match self.ext_vars.get(&key).copied() {
                                        Some(val) => {
                                            self.push(val)?;
                                            continue;
                                        }
                                        None => {
                                            return Err(RuntimeError::new(
                                                span,
                                                format!("Undefined external variable: '{}'", key),
                                                source_id,
                                            ));
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setInter: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let b_idx = match b_val {
                                        Value::Array(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setInter: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setDiff: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let b_idx = match b_val {
                                        Value::Array(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setDiff: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setMember: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                                upvalue =
                                                    self.memory_manager.load_upvalue(uv_idx).next;
                                            }
                                            self.memory_manager
                                                .push_external_roots(roots, open_upvalue_roots);
                                            let k =
                                                self.call_value_with_one_arg(key_f, arr_elems[mid]);
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setUnion: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let b_idx = match b_val {
                                        Value::Array(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "setUnion: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                            if native::compare_values(
                                                lk,
                                                *key,
                                                &self.memory_manager,
                                            ) == std::cmp::Ordering::Equal
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

                                // Handle std.sort with keyF
                                if id == chunk::NativeFuncId::Sort && args.len() == 2 {
                                    let arr_val = args[0];
                                    let key_f = args[1];
                                    let arr_idx = match arr_val {
                                        Value::Array(a) => a,
                                        _ => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                "std.sort() expected array as first argument"
                                                    .to_string(),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                    indexed.sort_by(|&a, &b| {
                                        native::compare_values(keys[a], keys[b], mm)
                                    });
                                    let sorted: Vec<Value> =
                                        indexed.iter().map(|&i| elements[i]).collect();
                                    let arr_alloc = self.memory_manager.allocate_array(sorted);
                                    self.push(Value::Array(arr_alloc.index))?;
                                    continue;
                                }

                                // Handle std.groupBy
                                if id == chunk::NativeFuncId::GroupBy {
                                    let arr_val = args[0];
                                    let key_f = args[1];
                                    let arr_idx = match arr_val {
                                        Value::Array(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.groupBy: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!(
                                                        "std.groupBy: keyF must return string, got {}",
                                                        other.type_name()
                                                    ),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
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
                                        let group_elems =
                                            groups.remove(key_str).unwrap_or_default();
                                        let arr_alloc =
                                            self.memory_manager.allocate_array(group_elems);
                                        let k_alloc = self.memory_manager.allocate_string(key_str);
                                        properties.insert(
                                            k_alloc.index,
                                            memory_manager::ObjectField::new(
                                                Value::Array(arr_alloc.index),
                                                FieldVisibility::Visible,
                                            ),
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.sortBy: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.countBy: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!(
                                                        "std.countBy: keyF must return string, got {}",
                                                        other.type_name()
                                                    ),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
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
                                            memory_manager::ObjectField::new(
                                                Value::Number(count as f64),
                                                FieldVisibility::Visible,
                                            ),
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.uniqBy: expected array, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
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
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!(
                                                        "std.uniqBy: keyF must return string, got {}",
                                                        other.type_name()
                                                    ),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
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
                                if id == chunk::NativeFuncId::MinBy
                                    || id == chunk::NativeFuncId::MaxBy
                                {
                                    let arr_val = args[0];
                                    let key_f = args[1];
                                    let arr_idx = match arr_val {
                                        Value::Array(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.{}: expected array, got {}",
                                                    id.name(),
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    // Root keyF before forcing array elements (which may trigger GC)
                                    self.memory_manager
                                        .external_roots
                                        .push(vec![key_f, arr_val]);
                                    self.force_all_array_elements(arr_idx)?;
                                    let elements: Vec<Value> =
                                        self.memory_manager.load_array(arr_idx).elements.clone();
                                    if elements.is_empty() {
                                        self.memory_manager.external_roots.pop();
                                        return Err(RuntimeError::new(
                                            self.get_current_span(),
                                            format!("std.{}: array must not be empty", id.name()),
                                            self.current_chunk().source_id.to_string(),
                                        ));
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
                                        let cmp = native::compare_values(
                                            key,
                                            best_key,
                                            &self.memory_manager,
                                        );
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
                                    self.memory_manager.external_roots.pop(); // pop keyF/arrVal roots
                                    self.push(best_elem)?;
                                    continue;
                                }

                                // Handle std.toPairs
                                if id == chunk::NativeFuncId::ToPairs {
                                    let obj_val = args[0];
                                    let o_idx = match obj_val {
                                        Value::Object(i) => i,
                                        other => {
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.toPairs: expected object, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let field_data: Vec<(chunk::StringIndex, Value)> = self
                                        .enumerate_object_fields(o_idx)
                                        .into_iter()
                                        .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                                        .map(|(k, v, _, _)| (k, v))
                                        .collect();
                                    let mut pairs: Vec<Value> =
                                        Vec::with_capacity(field_data.len());
                                    for (k_idx, raw_val) in &field_data {
                                        let k_idx = *k_idx;
                                        let raw_val = *raw_val;
                                        let evaled_val = match raw_val {
                                            Value::Closure(closure_idx) => self
                                                .execute_thunk_sync(
                                                    closure_idx,
                                                    Some(o_idx),
                                                    None,
                                                )?,
                                            other => other,
                                        };
                                        let key_str =
                                            self.memory_manager.load_string(k_idx).to_string();
                                        let k_alloc = self.memory_manager.allocate_string(&key_str);
                                        let k_val = Value::String(k_alloc.index);
                                        let pair_alloc = self
                                            .memory_manager
                                            .allocate_array(vec![k_val, evaled_val]);
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.mapKeys: expected object, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let field_data: Vec<(
                                        chunk::StringIndex,
                                        Value,
                                        FieldVisibility,
                                    )> = self
                                        .enumerate_object_fields(o_idx)
                                        .into_iter()
                                        .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                                        .map(|(k, v, _, vis)| (k, v, vis))
                                        .collect();
                                    let mut new_properties: std::collections::HashMap<
                                        chunk::StringIndex,
                                        memory_manager::ObjectField,
                                    > = std::collections::HashMap::new();
                                    for (k_idx, raw_val, vis) in &field_data {
                                        let k_idx = *k_idx;
                                        let raw_val = *raw_val;
                                        let vis = *vis;
                                        let evaled_val = match raw_val {
                                            Value::Closure(closure_idx) => self
                                                .execute_thunk_sync(
                                                    closure_idx,
                                                    Some(o_idx),
                                                    None,
                                                )?,
                                            other => other,
                                        };
                                        let key_str =
                                            self.memory_manager.load_string(k_idx).to_string();
                                        let key_alloc =
                                            self.memory_manager.allocate_string(&key_str);
                                        let key_val = Value::String(key_alloc.index);
                                        let mut roots = Vec::from(self.stack.clone());
                                        roots.push(func_val);
                                        roots.push(evaled_val);
                                        roots.push(key_val);
                                        for (k, f) in new_properties.iter() {
                                            roots.push(Value::String(*k));
                                            roots.push(f.value);
                                        }
                                        roots.push(Value::Object(o_idx));
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
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!(
                                                        "std.mapKeys: func must return string, got {}",
                                                        other.type_name()
                                                    ),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
                                            }
                                        };
                                        let new_k_alloc =
                                            self.memory_manager.allocate_string(&new_key_str);
                                        new_properties.insert(
                                            new_k_alloc.index,
                                            memory_manager::ObjectField::new(evaled_val, vis),
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.filterObject: expected object, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let field_data: Vec<(
                                        chunk::StringIndex,
                                        Value,
                                        FieldVisibility,
                                    )> = self
                                        .enumerate_object_fields(o_idx)
                                        .into_iter()
                                        .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                                        .map(|(k, v, _, vis)| (k, v, vis))
                                        .collect();
                                    let mut kept_properties: std::collections::HashMap<
                                        chunk::StringIndex,
                                        memory_manager::ObjectField,
                                    > = std::collections::HashMap::new();
                                    for (k_idx, raw_val, vis) in &field_data {
                                        let k_idx = *k_idx;
                                        let raw_val = *raw_val;
                                        let vis = *vis;
                                        let evaled_val = match raw_val {
                                            Value::Closure(closure_idx) => self
                                                .execute_thunk_sync(
                                                    closure_idx,
                                                    Some(o_idx),
                                                    None,
                                                )?,
                                            other => other,
                                        };
                                        let key_str =
                                            self.memory_manager.load_string(k_idx).to_string();
                                        let key_alloc =
                                            self.memory_manager.allocate_string(&key_str);
                                        let key_val = Value::String(key_alloc.index);
                                        let mut roots = Vec::from(self.stack.clone());
                                        roots.push(func_val);
                                        roots.push(evaled_val);
                                        roots.push(key_val);
                                        for (k, f) in kept_properties.iter() {
                                            roots.push(Value::String(*k));
                                            roots.push(f.value);
                                        }
                                        roots.push(Value::Object(o_idx));
                                        let mut open_upvalue_roots = Vec::new();
                                        let mut upvalue = self.open_upvalues;
                                        while let Some(uv_idx) = upvalue {
                                            open_upvalue_roots.push(uv_idx);
                                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                        }
                                        self.memory_manager
                                            .push_external_roots(roots, open_upvalue_roots);
                                        let keep = self.call_value_with_two_args(
                                            func_val, key_val, evaled_val,
                                        );
                                        self.memory_manager.pop_external_roots();
                                        match keep? {
                                            Value::Boolean(true) => {
                                                kept_properties.insert(
                                                    k_idx,
                                                    memory_manager::ObjectField::new(
                                                        evaled_val, vis,
                                                    ),
                                                );
                                            }
                                            Value::Boolean(false) => {}
                                            other => {
                                                return Err(RuntimeError::new(
                                                    self.get_current_span(),
                                                    format!(
                                                        "std.filterObject: func must return bool, got {}",
                                                        other.type_name()
                                                    ),
                                                    self.current_chunk().source_id.to_string(),
                                                ));
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
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                format!(
                                                    "std.objectFlatten: sep must be string, got {}",
                                                    other.type_name()
                                                ),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    };
                                    let mut flat_fields: Vec<(String, Value)> = Vec::new();

                                    // Root arguments during recursive flattening
                                    self.memory_manager
                                        .push_external_roots(args.clone(), Vec::new());
                                    let flatten_res = self.flatten_object_recursive(
                                        obj_val,
                                        &sep,
                                        String::new(),
                                        &mut flat_fields,
                                    );
                                    self.memory_manager.pop_external_roots();
                                    flatten_res?;

                                    let mut properties: std::collections::HashMap<
                                        chunk::StringIndex,
                                        memory_manager::ObjectField,
                                    > = std::collections::HashMap::new();

                                    // Root the values in flat_fields while building the object
                                    let flat_vals: Vec<Value> =
                                        flat_fields.iter().map(|(_, v)| *v).collect();
                                    self.memory_manager
                                        .push_external_roots(flat_vals, Vec::new());

                                    for (k_str, v) in flat_fields {
                                        let k_alloc = self.memory_manager.allocate_string(&k_str);
                                        properties.insert(
                                            k_alloc.index,
                                            memory_manager::ObjectField::new(
                                                v,
                                                FieldVisibility::Visible,
                                            ),
                                        );
                                    }
                                    let obj_alloc = self
                                        .memory_manager
                                        .allocate_object_with_properties(properties);

                                    self.memory_manager.pop_external_roots();

                                    self.push(Value::Object(obj_alloc.index))?;

                                    if obj_alloc.should_garbage_collect {
                                        self.run_garbage_collection();
                                    }
                                    continue;
                                }

                                // Call native function
                                let span = self.get_current_span();
                                let source_id = self.current_chunk().source_id.to_string();
                                let result =
                                    self.call_native_checked(id, &args, span, source_id)?;
                                self.push(result)?;
                            }

                            _ => {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!("Cannot call non-function value: {:?}", callee),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                        } // close match callee
                    } // close else
                } // close Opcode::Call

                Opcode::TailCall => {
                    // Read operands: positional_count and named_count (same layout as Call)
                    let frame = self.current_frame();
                    let chunk = self.current_chunk();

                    if frame.ip + 2 >= chunk.count() {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Invalid bytecode - missing TailCall operands".to_string(),
                            chunk.source_id.to_string(),
                        ));
                    }

                    let positional_count = chunk.code[frame.ip + 1] as usize;
                    let named_count = chunk.code[frame.ip + 2] as usize;

                    // Advance IP past this instruction (opcode + 2 bytes)
                    self.current_frame_mut().ip += 3;

                    // If named args, resolve them first by rearranging the stack
                    let mut resolved_arg_count = positional_count;
                    if named_count > 0 {
                        let total_stack_items = positional_count + named_count * 2;
                        let callee_position = self.stack.len() - total_stack_items - 1;
                        let mut callee = self.stack[callee_position];
                        callee = self.force_value(callee)?;
                        self.stack[callee_position] = callee;

                        if let Value::Closure(closure_index) = callee {
                            let func_index =
                                self.memory_manager.load_closure(closure_index).function;
                            let function = self.memory_manager.load_function(func_index);
                            let arity = function.arity as usize;
                            let param_names = function.param_names.clone();

                            // Collect named args from stack
                            let mut named_args: Vec<(StringIndex, Value)> = Vec::new();
                            for _ in 0..named_count {
                                let value = self.pop()?;
                                let name_val = self.pop()?;
                                if let Value::String(name_idx) = name_val {
                                    named_args.push((name_idx, value));
                                } else {
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        "Named argument name must be a string".to_string(),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }

                            // Collect positional args
                            let mut positional_args: Vec<Value> = Vec::new();
                            for _ in 0..positional_count {
                                positional_args.push(self.pop()?);
                            }
                            positional_args.reverse();

                            // Pop callee
                            self.pop()?;

                            // Build full argument array
                            let mut args = vec![Value::Uninitialized; arity];
                            for (i, val) in positional_args.into_iter().enumerate() {
                                if i < arity {
                                    args[i] = val;
                                }
                            }
                            for (name_idx, val) in named_args {
                                if let Some(pos) = param_names.iter().position(|&pn| pn == name_idx)
                                {
                                    args[pos] = val;
                                }
                            }

                            // Push callee and reordered args back
                            self.push(Value::Closure(closure_index))?;
                            resolved_arg_count = arity;
                            for val in args {
                                self.push(val)?;
                            }
                        } else {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Named arguments only supported for closures".to_string(),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                    }

                    let arg_count = resolved_arg_count;

                    // Get callee from stack at position: stack.len() - arg_count - 1
                    let callee_position = self.stack.len() - arg_count - 1;

                    let mut callee = self.stack[callee_position];
                    callee = self.force_value(callee)?;
                    self.stack[callee_position] = callee;

                    // Force all arguments before destroying the current frame
                    let stack_len = self.stack.len();
                    for i in 0..arg_count {
                        let arg_pos = stack_len - arg_count + i;
                        let arg = self.stack[arg_pos];
                        let forced_arg = self.force_value(arg)?;
                        self.stack[arg_pos] = forced_arg;
                    }

                    match callee {
                        Value::Closure(closure_index) => {
                            // Validate arity before doing anything destructive
                            let (arity, required) = {
                                let closure = self.memory_manager.load_closure(closure_index);
                                let function = self.memory_manager.load_function(closure.function);
                                (function.arity as usize, function.required_params as usize)
                            };

                            if arg_count < required || arg_count > arity {
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "Function expects {}-{} argument(s), got {}",
                                        required, arity, arg_count
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }

                            // Pad missing optional arguments
                            for _ in arg_count..arity {
                                self.push(Value::Uninitialized)?;
                            }
                            let arg_count = arity;

                            // Copy the current frame's stack_base before mutating
                            let current_stack_base = self.current_frame().stack_base;

                            // Close upvalues for slots at or above current_stack_base
                            // (same as what return_from_function does)
                            self.close_upvalues(current_stack_base);

                            // Move [closure, arg0, ..., argN-1] from their current positions
                            // to start at current_stack_base, then truncate the stack.
                            let new_frame_start = self.stack.len() - arg_count - 1;
                            let item_count = arg_count + 1; // closure + args

                            // Copy the items into place
                            for i in 0..item_count {
                                self.stack[current_stack_base + i] =
                                    self.stack[new_frame_start + i];
                            }
                            self.stack.truncate(current_stack_base + item_count);

                            // Reuse the current frame: update its fields in-place
                            {
                                let frame = self.current_frame_mut();
                                frame.closure = closure_index;
                                frame.ip = 0;
                                frame.self_obj = None;
                                frame.super_obj = None;
                                // frame.stack_base stays the same
                            }

                            // Do NOT push a new frame — frame_count stays the same
                            continue;
                        }
                        Value::NativeFunction(id) => {
                            // Fall back to regular Call behaviour for native functions
                            let args = self.stack[self.stack.len() - arg_count..].to_vec();
                            for _ in 0..=arg_count {
                                self.pop()?;
                            }

                            if matches!(
                                id,
                                chunk::NativeFuncId::MergePatch
                                    | chunk::NativeFuncId::Prune
                                    | chunk::NativeFuncId::Uniq
                                    | chunk::NativeFuncId::Sort
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
                                    chunk::NativeFuncId::Sort => {
                                        let result =
                                            self.sort_value(args[0], args.get(1).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::Set => {
                                        let arr_val = args[0];
                                        if let Value::Array(a) = arr_val {
                                            self.force_all_array_elements(a)?;
                                        }
                                        let sorted = self.sort_value(arr_val, None)?;
                                        let result =
                                            self.uniq_value(sorted, args.get(1).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    chunk::NativeFuncId::SetUnion => {
                                        let a_val = args[0];
                                        let b_val = args[1];
                                        let a_idx =
                                            match a_val {
                                                Value::Array(i) => {
                                                    self.force_all_array_elements(i)?;
                                                    i
                                                }
                                                _ => return Err(RuntimeError::new(
                                                    span,
                                                    "std.setUnion: first argument must be an array"
                                                        .to_string(),
                                                    source_id,
                                                )),
                                            };
                                        let b_idx = match b_val {
                                            Value::Array(i) => {
                                                self.force_all_array_elements(i)?;
                                                i
                                            }
                                            _ => return Err(RuntimeError::new(
                                                span,
                                                "std.setUnion: second argument must be an array"
                                                    .to_string(),
                                                source_id,
                                            )),
                                        };
                                        let mut combined =
                                            self.memory_manager.load_array(a_idx).elements.clone();
                                        combined.extend_from_slice(
                                            &self.memory_manager.load_array(b_idx).elements.clone(),
                                        );
                                        let alloc = self.memory_manager.allocate_array(combined);
                                        let sorted =
                                            self.sort_value(Value::Array(alloc.index), None)?;
                                        let result =
                                            self.uniq_value(sorted, args.get(2).copied())?;
                                        self.push(result)?;
                                        continue;
                                    }
                                    _ => unreachable!(),
                                }
                            }

                            let span = self.get_current_span();
                            let source_id = self.current_chunk().source_id.to_string();
                            let result = self.call_native_checked(id, &args, span, source_id)?;
                            self.push(result)?;
                        }
                        _ => {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!("Cannot call non-function value: {:?}", callee),
                                self.current_chunk().source_id.to_string(),
                            ));
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

                    // Write field cache if this frame was called from ObjectIndex or SuperIndex
                    // Only cache non-Closure values (closures are methods, not thunk results)
                    if !matches!(return_value, Value::Closure(_)) {
                        let cache_target = self.current_frame().cache_target;
                        if let Some(cache_key) = cache_target {
                            self.field_cache.insert(cache_key, return_value);
                        }
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
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!(
                                "Invalid upvalue slot {} (closure has {} upvalues)",
                                upvalue_slot,
                                current_closure.upvalues.len()
                            ),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }

                    let upvalue_index = current_closure.upvalues[upvalue_slot];
                    let upvalue = self.memory_manager.load_upvalue(upvalue_index);

                    let value = if let Some(location) = upvalue.stack_location {
                        // Upvalue is open - read from stack
                        if location >= self.stack.len() {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                format!(
                                    "Invalid upvalue stack location {} (stack size: {})",
                                    location,
                                    self.stack.len()
                                ),
                                self.current_chunk().source_id.to_string(),
                            ));
                        }
                        let val = self.stack[location];
                        let res = self.force_value(val)?;
                        self.stack[location] = res;
                        res
                    } else if let Some(closed_value) = upvalue.closed_value {
                        // Upvalue is closed - read from heap
                        let res = self.force_value(closed_value)?;
                        let upvalue_mut = self.memory_manager.load_upvalue_mut(upvalue_index);
                        upvalue_mut.closed_value = Some(res);
                        res
                    } else {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Upvalue has neither stack location nor closed value".to_string(),
                            self.current_chunk().source_id.to_string(),
                        ));
                    };

                    self.push(value)?;
                }

                Opcode::CloseUpvalue => {
                    // Close upvalue for top of stack (does NOT pop - compiler emits separate Pop)
                    let stack_top = self.stack.len() - 1;
                    self.close_upvalues(stack_top);
                    self.advance_pc();
                }

                Opcode::Closure | Opcode::MakeThunk => {
                    let is_thunk = opcode == Opcode::MakeThunk;
                    // Read function index from constants
                    let func_index_in_constants = self.read_u16_operand()?;
                    let chunk = self.current_chunk();

                    if func_index_in_constants as usize >= chunk.constants.len() {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("Invalid constant index: {}", func_index_in_constants),
                            chunk.source_id.to_string(),
                        ));
                    }

                    let func_value = chunk.constants[func_index_in_constants as usize];
                    let func_index = if let Value::Function(idx) = func_value {
                        idx
                    } else {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("Expected function in constants, got {:?}", func_value),
                            chunk.source_id.to_string(),
                        ));
                    };

                    // Read upvalue count from bytecode
                    let frame = self.current_frame();
                    if frame.ip >= chunk.count() {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            "Invalid bytecode - missing upvalue count".to_string(),
                            chunk.source_id.to_string(),
                        ));
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
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Invalid bytecode - missing upvalue is_local flag".to_string(),
                                chunk.source_id.to_string(),
                            ));
                        }
                        let is_local = chunk.code[frame.ip] != 0;
                        self.current_frame_mut().ip += 1;

                        // Read index (u16)
                        let frame = self.current_frame();
                        let chunk = self.current_chunk();
                        if frame.ip + 1 >= chunk.count() {
                            return Err(RuntimeError::new(
                                self.get_current_span(),
                                "Invalid bytecode - missing upvalue index".to_string(),
                                chunk.source_id.to_string(),
                            ));
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
                                return Err(RuntimeError::new(
                                    self.get_current_span(),
                                    format!(
                                        "Invalid upvalue index {} (closure has {} upvalues)",
                                        index,
                                        current_closure.upvalues.len()
                                    ),
                                    self.current_chunk().source_id.to_string(),
                                ));
                            }
                            current_closure.upvalues[index]
                        };

                        upvalue_indices.push(upvalue_index);
                    }

                    // Create closure (or thunk)
                    let closure_allocation = if is_thunk {
                        self.memory_manager
                            .allocate_thunk(func_index, upvalue_indices)
                    } else {
                        self.memory_manager
                            .allocate_closure(func_index, upvalue_indices)
                    };
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

                Opcode::LoadStd => {
                    self.advance_pc();
                    let std_obj = self.get_or_create_std_object();
                    self.push(std_obj)?;
                }

                // All other opcodes result in runtime error
                _ => {
                    return Err(RuntimeError::new(
                        self.get_current_span(),
                        format!("Unimplemented opcode: {:?}", opcode),
                        self.current_chunk().source_id.to_string(),
                    ));
                }
            }
        }
    }

    /// Get or create the `std` object with all native functions as hidden fields.
    fn get_or_create_std_object(&mut self) -> Value {
        if let Some(obj) = self.std_object {
            return obj;
        }

        let mut properties = std::collections::HashMap::new();
        for &(name, id) in chunk::NativeFuncId::all_with_names() {
            let key = self.memory_manager.allocate_string(name).index;
            properties.insert(
                key,
                memory_manager::ObjectField::new(
                    Value::NativeFunction(id),
                    FieldVisibility::Hidden,
                ),
            );
        }
        let alloc = self
            .memory_manager
            .allocate_object_with_properties(properties);
        let obj = Value::Object(alloc.index);
        self.std_object = Some(obj);
        // Also register as a persistent external root so the object survives
        // GC runs triggered by the compiler (which doesn't know about VM state).
        self.memory_manager
            .push_external_roots(vec![obj], Vec::new());
        obj
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
                let a_len = self.memory_manager.load_array(*a_idx).elements.len();
                let b_len = self.memory_manager.load_array(*b_idx).elements.len();

                self.memory_manager
                    .external_roots
                    .push(vec![Value::Array(*a_idx)]);
                self.memory_manager
                    .external_roots
                    .push(vec![Value::Array(*b_idx)]);

                let res = (|| {
                    if a_len != b_len {
                        return Ok(false);
                    }
                    for i in 0..a_len {
                        let v_a = self.memory_manager.load_array(*a_idx).elements[i];
                        let v_a = self.force_array_element(*a_idx, i, v_a)?;
                        let v_b = self.memory_manager.load_array(*b_idx).elements[i];
                        let v_b = self.force_array_element(*b_idx, i, v_b)?;
                        if !self.values_equal(&v_a, &v_b)? {
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

    /// Compare two values for ordering according to Jsonnet semantics.
    /// Supports numbers, strings, and arrays. Returns an error for incomparable types.
    fn compare_values(&mut self, a: Value, b: Value) -> Result<std::cmp::Ordering, RuntimeError> {
        let a = self.force_value(a)?;
        let b = self.force_value(b)?;
        match (a, b) {
            (Value::Number(a), Value::Number(b)) => {
                Ok(a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal))
            }
            (Value::String(a), Value::String(b)) => {
                let a_str = self.memory_manager.load_string(a).to_string();
                let b_str = self.memory_manager.load_string(b).to_string();
                Ok(a_str.cmp(&b_str))
            }
            (Value::Array(a_idx), Value::Array(b_idx)) => {
                let a_len = self.memory_manager.load_array(a_idx).len();
                let b_len = self.memory_manager.load_array(b_idx).len();
                let min_len = a_len.min(b_len);
                for i in 0..min_len {
                    let a_elem = self.memory_manager.load_array(a_idx).elements[i];
                    let a_elem = self.force_array_element(a_idx, i, a_elem)?;
                    let b_elem = self.memory_manager.load_array(b_idx).elements[i];
                    let b_elem = self.force_array_element(b_idx, i, b_elem)?;
                    let ord = self.compare_values(a_elem, b_elem)?;
                    if ord != std::cmp::Ordering::Equal {
                        return Ok(ord);
                    }
                }
                Ok(a_len.cmp(&b_len))
            }
            _ => Err(RuntimeError::new(
                self.get_current_span(),
                format!("Cannot compare {:?} with {:?}", a, b),
                self.current_chunk().source_id.to_string(),
            )),
        }
    }

    /// Helper to get all visible fields of an object with their evaluated values.
    fn get_visible_fields(
        &mut self,
        obj_idx: ObjectIndex,
    ) -> Result<std::collections::HashMap<StringIndex, Value>, RuntimeError> {
        let mut visible_fields = std::collections::HashMap::new();

        let properties: Vec<(StringIndex, Value, Option<ObjectIndex>, FieldVisibility)> = self
            .enumerate_object_fields(obj_idx)
            .into_iter()
            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
            .collect();

        for (name, value, super_obj, _vis) in properties {
            let current_vals: Vec<Value> = visible_fields.values().cloned().collect();
            self.memory_manager.external_roots.push(current_vals);

            let val_res = match value {
                Value::Closure(closure_idx) => self.execute_thunk_sync_with_field(
                    closure_idx,
                    Some(obj_idx),
                    super_obj,
                    Some(name),
                ),
                other => Ok(other),
            };

            self.memory_manager.external_roots.pop();

            let val = val_res?;
            visible_fields.insert(name, val);
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
                    .ok_or_else(|| {
                        RuntimeError::new(
                            0..0,
                            "Invalid number for JSON conversion".to_string(),
                            "serialization".to_string(),
                        )
                    }),
                Value::String(s) => Ok(serde_json::Value::String(
                    self.memory_manager.load_string(s).to_owned(),
                )),
                Value::Object(object_key) => {
                    // Check deferred assertions during manifestation
                    self.check_object_assertions(object_key)?;
                    // Check for circular references
                    if visited.contains(&object_key) {
                        return Err(RuntimeError::new(
                            0..0,
                            "Circular reference detected in object".to_string(),
                            "serialization".to_string(),
                        ));
                    }

                    visited.insert(object_key);
                    let properties: Vec<(StringIndex, Value, Option<ObjectIndex>)> = self
                        .enumerate_object_fields(object_key)
                        .into_iter()
                        .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                        .map(|(k, v, so, _)| (k, v, so))
                        .collect();

                    let mut json_object = serde_json::Map::new();

                    for (key, value, super_obj) in properties {
                        let field_value = match value {
                            Value::Closure(closure_idx)
                                if self.memory_manager.load_closure(closure_idx).is_thunk =>
                            {
                                self.execute_thunk_sync_with_field(
                                    closure_idx,
                                    Some(object_key),
                                    super_obj,
                                    Some(key),
                                )?
                            }
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
                    // Force all thunked elements before serialization
                    self.force_all_array_elements(array_key)?;
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
                _ => Err(RuntimeError::new(
                    self.get_current_span(),
                    format!("Cannot serialize value to JSON: {:?}", value),
                    "serialization".to_string(),
                )),
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

        // Root the cached std object so GC doesn't collect it
        if let Some(std_obj) = self.std_object {
            roots.push(std_obj);
        }

        // Root all cached field values so GC doesn't collect them
        for (&(obj_key, field_key, self_key), &value) in &self.field_cache {
            roots.push(Value::Object(obj_key));
            roots.push(Value::String(field_key));
            roots.push(Value::Object(self_key));
            roots.push(value);
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
            Value::Object(x) => Ok(self
                .enumerate_object_fields(x)
                .iter()
                .any(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)),
            Value::Array(x) => Ok(self.memory_manager.load_array(x).len() > 0),
            Value::Binary(x) => Ok(self.memory_manager.load_binary(x).data.len() > 0),
            Value::Function(_) => Ok(true), // Functions are truthy
            Value::Closure(_) => Ok(true),  // Closures are truthy
            Value::NativeFunction(_) => Ok(true), // Native functions are truthy
            Value::NativeThunk(_) => Ok(true), // Thunks are truthy
            Value::Import(_) => Ok(true),   // Should be unreachable due to force_value
            Value::Uninitialized => Ok(false),
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
                    Err(e) => Err(RuntimeError::new(
                        self.get_current_span(),
                        format!("Failed to parse string {} to f64", e),
                        self.current_chunk().source_id.to_string(),
                    )),
                }
            }
            _ => Err(RuntimeError::new(
                self.get_current_span(),
                format!("Cannot convert {:?} to f64", value),
                self.current_chunk().source_id.to_string(),
            )),
        }
    }

    fn to_integer(&mut self, value: Value) -> Result<i64, RuntimeError> {
        let n = self.to_number(value)?;
        if n.is_nan() || n.is_infinite() {
            Err(RuntimeError::new(
                self.get_current_span(),
                "Cannot convert NaN or Infinity to integer".to_string(),
                self.current_chunk().source_id.to_string(),
            ))
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
                        memory_manager::ObjectField::new(val, chunk::FieldVisibility::Visible),
                    );
                }
                let alloc = self.memory_manager.allocate_object_with_properties(props);
                Ok(Value::Object(alloc.index))
            }
        }
    }

    /// Parse a JSON string into a Jsonnet Value using Rust's f64 parser for number consistency.
    fn parse_json_value(
        &mut self,
        input: &str,
        span: Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        let mut pos = 0;
        let bytes = input.as_bytes();
        let result = self.parse_json_inner(bytes, &mut pos, &span, source_id)?;
        // Skip trailing whitespace
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos != bytes.len() {
            return Err(RuntimeError::new(
                span.clone(),
                format!(
                    "std.parseJson: unexpected trailing content at position {}",
                    pos
                ),
                source_id.to_string(),
            ));
        }
        Ok(result)
    }

    fn parse_json_inner(
        &mut self,
        bytes: &[u8],
        pos: &mut usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        self.skip_json_ws(bytes, pos);
        if *pos >= bytes.len() {
            return Err(RuntimeError::new(
                span.clone(),
                "std.parseJson: unexpected end of input".to_string(),
                source_id.to_string(),
            ));
        }
        match bytes[*pos] {
            b'n' => {
                if bytes[*pos..].starts_with(b"null") {
                    *pos += 4;
                    Ok(Value::Null)
                } else {
                    Err(RuntimeError::new(
                        span.clone(),
                        "std.parseJson: expected 'null'".to_string(),
                        source_id.to_string(),
                    ))
                }
            }
            b't' => {
                if bytes[*pos..].starts_with(b"true") {
                    *pos += 4;
                    Ok(Value::Boolean(true))
                } else {
                    Err(RuntimeError::new(
                        span.clone(),
                        "std.parseJson: expected 'true'".to_string(),
                        source_id.to_string(),
                    ))
                }
            }
            b'f' => {
                if bytes[*pos..].starts_with(b"false") {
                    *pos += 5;
                    Ok(Value::Boolean(false))
                } else {
                    Err(RuntimeError::new(
                        span.clone(),
                        "std.parseJson: expected 'false'".to_string(),
                        source_id.to_string(),
                    ))
                }
            }
            b'"' => self.parse_json_string(bytes, pos, span, source_id),
            b'[' => self.parse_json_array(bytes, pos, span, source_id),
            b'{' => self.parse_json_object(bytes, pos, span, source_id),
            b'-' | b'0'..=b'9' => self.parse_json_number(bytes, pos, span, source_id),
            c => Err(RuntimeError::new(
                span.clone(),
                format!("std.parseJson: unexpected character '{}'", c as char),
                source_id.to_string(),
            )),
        }
    }

    fn skip_json_ws(&self, bytes: &[u8], pos: &mut usize) {
        while *pos < bytes.len() && bytes[*pos].is_ascii_whitespace() {
            *pos += 1;
        }
    }

    fn parse_json_number(
        &mut self,
        bytes: &[u8],
        pos: &mut usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        let start = *pos;
        if *pos < bytes.len() && bytes[*pos] == b'-' {
            *pos += 1;
        }
        while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
            *pos += 1;
        }
        if *pos < bytes.len() && bytes[*pos] == b'.' {
            *pos += 1;
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
        }
        if *pos < bytes.len() && (bytes[*pos] == b'e' || bytes[*pos] == b'E') {
            *pos += 1;
            if *pos < bytes.len() && (bytes[*pos] == b'+' || bytes[*pos] == b'-') {
                *pos += 1;
            }
            while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
                *pos += 1;
            }
        }
        let num_str = std::str::from_utf8(&bytes[start..*pos]).unwrap_or("");
        num_str.parse::<f64>().map(Value::Number).map_err(|_| {
            RuntimeError::new(
                span.clone(),
                format!("std.parseJson: invalid number '{}'", num_str),
                source_id.to_string(),
            )
        })
    }

    fn parse_json_string(
        &mut self,
        bytes: &[u8],
        pos: &mut usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        *pos += 1; // skip opening "
        let mut s = String::new();
        while *pos < bytes.len() && bytes[*pos] != b'"' {
            if bytes[*pos] == b'\\' {
                *pos += 1;
                if *pos >= bytes.len() {
                    return Err(RuntimeError::new(
                        span.clone(),
                        "std.parseJson: unterminated string escape".to_string(),
                        source_id.to_string(),
                    ));
                }
                match bytes[*pos] {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'b' => s.push('\u{08}'),
                    b'f' => s.push('\u{0c}'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    b'u' => {
                        *pos += 1;
                        if *pos + 4 > bytes.len() {
                            return Err(RuntimeError::new(
                                span.clone(),
                                "std.parseJson: incomplete unicode escape".to_string(),
                                source_id.to_string(),
                            ));
                        }
                        let hex = std::str::from_utf8(&bytes[*pos..*pos + 4]).unwrap_or("");
                        let cp = u32::from_str_radix(hex, 16).map_err(|_| {
                            RuntimeError::new(
                                span.clone(),
                                format!("std.parseJson: invalid unicode escape \\u{}", hex),
                                source_id.to_string(),
                            )
                        })?;
                        *pos += 4;
                        // Handle surrogate pairs: \uD800-\uDBFF followed by \uDC00-\uDFFF
                        if (0xD800..=0xDBFF).contains(&cp)
                            && *pos + 6 <= bytes.len()
                            && bytes[*pos] == b'\\'
                            && bytes[*pos + 1] == b'u'
                        {
                            let hex2 =
                                std::str::from_utf8(&bytes[*pos + 2..*pos + 6]).unwrap_or("");
                            if let Ok(cp2) = u32::from_str_radix(hex2, 16) {
                                if (0xDC00..=0xDFFF).contains(&cp2) {
                                    let full_cp = 0x10000 + ((cp - 0xD800) << 10) + (cp2 - 0xDC00);
                                    if let Some(c) = char::from_u32(full_cp) {
                                        s.push(c);
                                    }
                                    *pos += 6;
                                    // continue skips the *pos += 1 at loop end, no adjustment needed
                                    continue;
                                }
                            }
                        }
                        if let Some(c) = char::from_u32(cp) {
                            s.push(c);
                        }
                        *pos -= 1; // will be incremented below
                    }
                    c => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            format!("std.parseJson: invalid escape '\\{}'", c as char),
                            source_id.to_string(),
                        ));
                    }
                }
            } else {
                // Handle UTF-8 multi-byte sequences
                let remaining = &bytes[*pos..];
                if let Some(ch) = std::str::from_utf8(remaining)
                    .ok()
                    .and_then(|s| s.chars().next())
                {
                    s.push(ch);
                    *pos += ch.len_utf8() - 1; // -1 because of the += 1 below
                } else {
                    s.push(bytes[*pos] as char);
                }
            }
            *pos += 1;
        }
        if *pos >= bytes.len() {
            return Err(RuntimeError::new(
                span.clone(),
                "std.parseJson: unterminated string".to_string(),
                source_id.to_string(),
            ));
        }
        *pos += 1; // skip closing "
        let alloc = self.memory_manager.allocate_string(&s);
        Ok(Value::String(alloc.index))
    }

    fn parse_json_array(
        &mut self,
        bytes: &[u8],
        pos: &mut usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        *pos += 1; // skip [
        let mut elements = Vec::new();
        self.skip_json_ws(bytes, pos);
        if *pos < bytes.len() && bytes[*pos] == b']' {
            *pos += 1;
            let alloc = self.memory_manager.allocate_array(elements);
            return Ok(Value::Array(alloc.index));
        }
        loop {
            let val = self.parse_json_inner(bytes, pos, span, source_id)?;
            elements.push(val);
            self.skip_json_ws(bytes, pos);
            if *pos >= bytes.len() {
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.parseJson: unterminated array".to_string(),
                    source_id.to_string(),
                ));
            }
            if bytes[*pos] == b']' {
                *pos += 1;
                break;
            }
            if bytes[*pos] != b',' {
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.parseJson: expected ',' or ']' in array".to_string(),
                    source_id.to_string(),
                ));
            }
            *pos += 1; // skip comma
        }
        let alloc = self.memory_manager.allocate_array(elements);
        Ok(Value::Array(alloc.index))
    }

    fn parse_json_object(
        &mut self,
        bytes: &[u8],
        pos: &mut usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        *pos += 1; // skip {
        let mut props = std::collections::HashMap::new();
        self.skip_json_ws(bytes, pos);
        if *pos < bytes.len() && bytes[*pos] == b'}' {
            *pos += 1;
            let alloc = self.memory_manager.allocate_object_with_properties(props);
            return Ok(Value::Object(alloc.index));
        }
        loop {
            self.skip_json_ws(bytes, pos);
            let key_val = self.parse_json_string(bytes, pos, span, source_id)?;
            let key_idx = if let Value::String(idx) = key_val {
                idx
            } else {
                unreachable!()
            };
            self.skip_json_ws(bytes, pos);
            if *pos >= bytes.len() || bytes[*pos] != b':' {
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.parseJson: expected ':'".to_string(),
                    source_id.to_string(),
                ));
            }
            *pos += 1; // skip :
            let val = self.parse_json_inner(bytes, pos, span, source_id)?;
            props.insert(
                key_idx,
                memory_manager::ObjectField::new(val, chunk::FieldVisibility::Visible),
            );
            self.skip_json_ws(bytes, pos);
            if *pos >= bytes.len() {
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.parseJson: unterminated object".to_string(),
                    source_id.to_string(),
                ));
            }
            if bytes[*pos] == b'}' {
                *pos += 1;
                break;
            }
            if bytes[*pos] != b',' {
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.parseJson: expected ',' or '}' in object".to_string(),
                    source_id.to_string(),
                ));
            }
            *pos += 1; // skip comma
        }
        let alloc = self.memory_manager.allocate_object_with_properties(props);
        Ok(Value::Object(alloc.index))
    }

    /// Convert a value to a string for use in `+` string concatenation
    /// and `std.toString`. Produces compact single-line JSON for objects/arrays.
    fn value_to_string_for_concat(&mut self, val: &Value) -> Result<String, RuntimeError> {
        // Force thunks/imports first
        let val = match val {
            Value::Closure(c) => {
                if self.memory_manager.load_closure(*c).is_thunk {
                    self.force_thunk(*c)?
                } else {
                    *val
                }
            }
            Value::Import(_) => self.force_value(*val)?,
            other => *other,
        };
        self.value_to_string_inner(&val)
    }

    fn value_to_string_inner(&mut self, val: &Value) -> Result<String, RuntimeError> {
        match val {
            Value::String(s) => Ok(self.memory_manager.load_string(*s).to_string()),
            Value::Number(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", *n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Null => Ok("null".to_string()),
            Value::Array(a_idx) => {
                self.force_all_array_elements(*a_idx)?;
                let elements = self.memory_manager.load_array(*a_idx).elements.clone();
                if elements.is_empty() {
                    return Ok("[ ]".to_string());
                }
                let mut items = Vec::with_capacity(elements.len());
                for elem in &elements {
                    self.memory_manager
                        .push_external_roots(vec![*elem], Vec::new());
                    let s = self.value_to_string_json_value(elem)?;
                    self.memory_manager.pop_external_roots();
                    items.push(s);
                }
                Ok(format!("[{}]", items.join(", ")))
            }
            Value::Object(o_idx) => {
                self.check_object_assertions(*o_idx)?;
                let mut sorted_fields: Vec<(String, Value, Option<ObjectIndex>)> = self
                    .enumerate_object_fields(*o_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                sorted_fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

                if sorted_fields.is_empty() {
                    return Ok("{ }".to_string());
                }
                let mut pairs = Vec::with_capacity(sorted_fields.len());
                for (key_str, field_val, super_obj) in &sorted_fields {
                    let forced_val = match field_val {
                        Value::Closure(c) => {
                            self.memory_manager
                                .push_external_roots(vec![*field_val], Vec::new());
                            let result = self.execute_thunk_sync(*c, Some(*o_idx), *super_obj);
                            self.memory_manager.pop_external_roots();
                            result?
                        }
                        other => *other,
                    };
                    self.memory_manager
                        .push_external_roots(vec![forced_val], Vec::new());
                    let val_s = self.value_to_string_json_value(&forced_val)?;
                    self.memory_manager.pop_external_roots();
                    pairs.push(format!("\"{}\": {}", Self::json_escape_str(key_str), val_s));
                }
                Ok(format!("{{{}}}", pairs.join(", ")))
            }
            Value::Closure(c) => {
                if self.memory_manager.load_closure(*c).is_thunk {
                    let forced = self.force_thunk(*c)?;
                    self.value_to_string_inner(&forced)
                } else {
                    Ok("<<function>>".to_string())
                }
            }
            Value::Import(_) => {
                let forced = self.force_value(*val)?;
                self.value_to_string_inner(&forced)
            }
            Value::NativeFunction(_) | Value::Function(_) | Value::NativeThunk(_) => {
                Ok("<<function>>".to_string())
            }
            Value::Binary(_) | Value::Uninitialized => Ok("<<internal>>".to_string()),
        }
    }

    /// Produce compact JSON representation of a value (for use inside arrays/objects in toString)
    fn value_to_string_json_value(&mut self, val: &Value) -> Result<String, RuntimeError> {
        // Force thunks first
        let val = match val {
            Value::Closure(c) => {
                if self.memory_manager.load_closure(*c).is_thunk {
                    self.force_thunk(*c)?
                } else {
                    *val
                }
            }
            Value::Import(_) => self.force_value(*val)?,
            other => *other,
        };
        match val {
            Value::String(s_idx) => {
                let s = self.memory_manager.load_string(s_idx).to_string();
                Ok(format!("\"{}\"", Self::json_escape_str(&s)))
            }
            // For all other types, toString and JSON representation are the same
            _ => self.value_to_string_inner(&val),
        }
    }

    fn json_escape_str(s: &str) -> String {
        let mut out = String::new();
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out
    }

    /// Replace empty collection patterns "[\n\n<ws>]" → "[ ]" and "{\n\n<ws>}" → "{ }"
    /// for manifestJson (which differs from manifestJsonEx in empty collection handling).
    fn fix_manifest_json_empties(s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = Vec::with_capacity(chars.len());
        let mut i = 0;
        while i < chars.len() {
            if (chars[i] == '[' || chars[i] == '{')
                && i + 2 < chars.len()
                && chars[i + 1] == '\n'
                && chars[i + 2] == '\n'
            {
                let open = chars[i];
                let close = if open == '[' { ']' } else { '}' };
                let mut j = i + 3;
                while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                    j += 1;
                }
                if j < chars.len() && chars[j] == close {
                    out.push(open);
                    out.push(' ');
                    out.push(close);
                    i = j + 1;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out.into_iter().collect()
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
        // Root the current value
        self.memory_manager
            .push_external_roots(vec![value], Vec::new());

        let res = (|| -> Result<String, RuntimeError> {
            // Force closures/imports
            let value = self.force_value(value)?;
            let value = match value {
                Value::Closure(c) => {
                    if self.memory_manager.load_closure(c).is_thunk {
                        self.force_thunk(c)?
                    } else {
                        self.execute_thunk_sync(c, None, None)?
                    }
                }
                other => other,
            };

            match value {
                Value::Null => Ok("null".to_string()),
                Value::Boolean(true) => Ok("true".to_string()),
                Value::Boolean(false) => Ok("false".to_string()),
                Value::Number(n) => {
                    if n.is_nan() {
                        return Err(RuntimeError::new(
                            span,
                            "std.manifestJson: cannot serialize NaN".to_string(),
                            source_id.to_string(),
                        ));
                    }
                    if n.is_infinite() {
                        return Err(RuntimeError::new(
                            span,
                            "std.manifestJson: cannot serialize Infinite".to_string(),
                            source_id.to_string(),
                        ));
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
                    // Force all thunked elements before iterating
                    self.force_all_array_elements(a_idx)?;
                    let elements = self.memory_manager.load_array(a_idx).elements.clone();
                    let close_indent = indent.repeat(depth);
                    if elements.is_empty() {
                        return Ok(format!("[{}{}{}]", newline, newline, close_indent));
                    }
                    let item_indent = indent.repeat(depth + 1);
                    let mut items = Vec::with_capacity(elements.len());
                    for elem in elements {
                        // Root elem before recursive call
                        self.memory_manager
                            .push_external_roots(vec![elem], Vec::new());
                        let s = self.manifest_json_value(
                            elem,
                            indent,
                            newline,
                            key_val_sep,
                            depth + 1,
                            span.clone(),
                            source_id,
                        );
                        self.memory_manager.pop_external_roots();
                        let s = s?;
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
                    // Check deferred assertions during manifestation
                    self.check_object_assertions(o_idx)?;
                    // Collect visible fields from the full chain
                    let mut sorted_fields: Vec<(String, Value, Option<ObjectIndex>)> = self
                        .enumerate_object_fields(o_idx)
                        .into_iter()
                        .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                        .map(|(k, v, so, _)| {
                            (self.memory_manager.load_string(k).to_string(), v, so)
                        })
                        .collect();
                    sorted_fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

                    if sorted_fields.is_empty() {
                        let close_indent = indent.repeat(depth);
                        return Ok(format!("{{{}{}{}}}", newline, newline, close_indent));
                    }

                    let item_indent = indent.repeat(depth + 1);
                    let close_indent = indent.repeat(depth);
                    let mut pairs = Vec::with_capacity(sorted_fields.len());
                    for (key_str, field_val, super_obj) in sorted_fields {
                        // Force field value (thunk)
                        let forced_val = match field_val {
                            Value::Closure(c) => {
                                // Root everything needed before calling thunk
                                let roots = vec![field_val];
                                self.memory_manager.push_external_roots(roots, Vec::new());
                                let result = self.execute_thunk_sync(c, Some(o_idx), super_obj);
                                self.memory_manager.pop_external_roots();
                                result?
                            }
                            other => other,
                        };

                        // Root forced_val before recursive call
                        self.memory_manager
                            .push_external_roots(vec![forced_val], Vec::new());
                        let val_s = self.manifest_json_value(
                            forced_val,
                            indent,
                            newline,
                            key_val_sep,
                            depth + 1,
                            span.clone(),
                            source_id,
                        );
                        self.memory_manager.pop_external_roots();
                        let val_s = val_s?;
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
                _ => Err(RuntimeError::new(
                    span,
                    "std.manifestJson: cannot manifest function".to_string(),
                    source_id.to_string(),
                )),
            }
        })();

        self.memory_manager.pop_external_roots();
        res
    }

    fn flatten_object_recursive(
        &mut self,
        value: Value,
        sep: &str,
        prefix: String,
        out: &mut Vec<(String, Value)>,
    ) -> Result<(), RuntimeError> {
        // Root the current value being processed
        self.memory_manager
            .push_external_roots(vec![value], Vec::new());

        let res = match value {
            Value::Object(o_idx) => {
                let fields: Vec<(chunk::StringIndex, Value)> = self
                    .enumerate_object_fields(o_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != chunk::FieldVisibility::Hidden)
                    .map(|(k, v, _, _)| (k, v))
                    .collect();

                let mut loop_res = Ok(());
                for (k_idx, raw_val) in &fields {
                    let k_idx = *k_idx;
                    let raw_val = *raw_val;
                    let k_str = self.memory_manager.load_string(k_idx).to_string();
                    let full_key = if prefix.is_empty() {
                        k_str.clone()
                    } else {
                        format!("{}{}{}", prefix, sep, k_str)
                    };

                    // Protect accumulated out values and ALL remaining fields from GC.
                    // This is crucial because any evaluation might trigger GC.
                    let mut temp_vals: Vec<Value> = out.iter().map(|(_, v)| *v).collect();
                    for (_, fv) in fields.iter() {
                        temp_vals.push(*fv);
                    }
                    self.memory_manager
                        .push_external_roots(temp_vals, Vec::new());

                    let forced_v = match raw_val {
                        Value::Closure(closure_idx) => {
                            let result = self.execute_thunk_sync(closure_idx, Some(o_idx), None);
                            match result {
                                Ok(v) => v,
                                Err(e) => {
                                    self.memory_manager.pop_external_roots();
                                    loop_res = Err(e);
                                    break;
                                }
                            }
                        }
                        other => other,
                    };

                    let rec_res = self.flatten_object_recursive(forced_v, sep, full_key, out);
                    self.memory_manager.pop_external_roots();

                    if let Err(e) = rec_res {
                        loop_res = Err(e);
                        break;
                    }
                }
                loop_res
            }
            other => {
                out.push((prefix, other));
                Ok(())
            }
        };

        self.memory_manager.pop_external_roots();
        res
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
                    // Walk the chain to get all fields from target
                    self.enumerate_object_fields(t_idx)
                        .into_iter()
                        .map(|(k, v, _super_obj, vis)| {
                            (k, memory_manager::ObjectField::new(v, vis))
                        })
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
                    memory_manager::ObjectField::new(merged, field.visibility),
                );
            }
        }
        let alloc = self.memory_manager.allocate_object_with_properties(result);
        Ok(Value::Object(alloc.index))
    }

    fn prune_value(&mut self, val: Value) -> Result<Value, RuntimeError> {
        // Force thunks first
        let val = match val {
            Value::Closure(c) if self.memory_manager.load_closure(c).is_thunk => {
                self.force_thunk(c)?
            }
            other => other,
        };
        match val {
            Value::Array(a_idx) => {
                self.force_all_array_elements(a_idx)?;
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
                // Root the object and all field values to protect from GC
                self.memory_manager
                    .push_external_roots(vec![Value::Object(o_idx)], Vec::new());
                let field_data: Vec<(
                    StringIndex,
                    Value,
                    Option<ObjectIndex>,
                    chunk::FieldVisibility,
                )> = self
                    .enumerate_object_fields(o_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != chunk::FieldVisibility::Hidden)
                    .collect();
                let field_vals: Vec<Value> = field_data.iter().map(|(_, v, _, _)| *v).collect();
                self.memory_manager
                    .push_external_roots(field_vals, Vec::new());
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
                        eval_v = self.execute_thunk_sync(closure_idx, Some(o_idx), super_obj)?;
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
                        new_props.insert(k, memory_manager::ObjectField::new(pruned_v, vis));
                    }
                }
                self.memory_manager.pop_external_roots(); // field values
                self.memory_manager.pop_external_roots(); // object
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

    fn sort_value(
        &mut self,
        arr_val: Value,
        key_func_opt: Option<Value>,
    ) -> Result<Value, RuntimeError> {
        // For the no-keyF case, force all thunks in the array before sorting.
        // Root arr_val and key_func_opt to protect them from GC during forcing.
        if matches!(key_func_opt, None | Some(Value::Null)) {
            if let Value::Array(arr_idx) = arr_val {
                let mut roots = vec![arr_val];
                if let Some(kf) = key_func_opt {
                    roots.push(kf);
                }
                self.memory_manager.external_roots.push(roots);
                let result = self.force_all_array_elements(arr_idx);
                self.memory_manager.external_roots.pop();
                result?;
            }
        }
        match key_func_opt {
            None | Some(Value::Null) => {
                // No keyF: delegate to the type-ordering sort in native
                let span = self.get_current_span();
                let source_id = self.current_chunk().source_id.to_string();
                native::std_sort(arr_val, &mut self.memory_manager, span, source_id)
            }
            Some(key_f) => {
                // keyF provided: pre-compute keys (same pattern as SortBy)
                let arr_idx = match arr_val {
                    Value::Array(i) => i,
                    other => {
                        return Err(RuntimeError::new(
                            self.get_current_span(),
                            format!("std.sort: expected array, got {}", other.type_name()),
                            self.current_chunk().source_id.to_string(),
                        ));
                    }
                };
                let elements: Vec<Value> = self.memory_manager.load_array(arr_idx).elements.clone();
                // Phase 1: pre-compute keys (GC-safe)
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
                Ok(Value::Array(alloc.index))
            }
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
                return Err(RuntimeError::new(
                    self.get_current_span(),
                    "std.uniq: first argument must be an array".to_string(),
                    self.current_chunk().source_id.to_string(),
                ));
            }
        };
        self.force_all_array_elements(arr_idx)?;
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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "std.manifestIni: expected object, got {}",
                        other.type_name()
                    ),
                    source_id,
                ));
            }
        };

        let mut result = String::new();

        // Root the top-level object so it survives GC during processing
        self.memory_manager
            .push_external_roots(vec![Value::Object(obj_idx)], Vec::new());

        // Process "main" section (no header)
        let main_val = self
            .enumerate_object_fields(obj_idx)
            .into_iter()
            .find(|(k, _, _, vis)| {
                self.memory_manager.load_string(*k) == "main" && *vis != FieldVisibility::Hidden
            })
            .map(|(_, v, so, _)| (v, so));
        if let Some((main_raw, main_super)) = main_val {
            let main_forced = match main_raw {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), main_super)?,
                other => other,
            };
            let main_forced = self.force_value(main_forced)?;
            let main_obj_idx = match main_forced {
                Value::Object(idx) => idx,
                other => {
                    self.memory_manager.pop_external_roots(); // pop obj_idx root
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "std.manifestIni: 'main' must be an object, got {}",
                            other.type_name()
                        ),
                        source_id,
                    ));
                }
            };
            // Root main_obj_idx
            self.memory_manager
                .push_external_roots(vec![Value::Object(main_obj_idx)], Vec::new());
            let mut main_fields: Vec<(String, Value, Option<ObjectIndex>)> = self
                .enumerate_object_fields(main_obj_idx)
                .into_iter()
                .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                .collect();
            main_fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
            for (key, val, super_obj) in main_fields {
                let forced_val = match val {
                    Value::Closure(c) => {
                        self.execute_thunk_sync(c, Some(main_obj_idx), super_obj)?
                    }
                    other => other,
                };
                let forced_val = self.force_value(forced_val)?;
                self.ini_field_to_string(&key, forced_val, &mut result, span.clone(), &source_id)?;
            }
            self.memory_manager.pop_external_roots(); // pop main_obj_idx root
        }

        // Process named sections
        let sections_val = self
            .enumerate_object_fields(obj_idx)
            .into_iter()
            .find(|(k, _, _, vis)| {
                self.memory_manager.load_string(*k) == "sections" && *vis != FieldVisibility::Hidden
            })
            .map(|(_, v, so, _)| (v, so));
        if let Some((sections_raw, sections_super)) = sections_val {
            let sections_forced = match sections_raw {
                Value::Closure(c) => self.execute_thunk_sync(c, Some(obj_idx), sections_super)?,
                other => other,
            };
            let sections_forced = self.force_value(sections_forced)?;
            let sections_obj_idx = match sections_forced {
                Value::Object(idx) => idx,
                other => {
                    self.memory_manager.pop_external_roots(); // pop obj_idx root
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "std.manifestIni: 'sections' must be an object, got {}",
                            other.type_name()
                        ),
                        source_id,
                    ));
                }
            };
            // Root sections_obj_idx
            self.memory_manager
                .push_external_roots(vec![Value::Object(sections_obj_idx)], Vec::new());
            let mut section_names: Vec<(String, Value, Option<ObjectIndex>)> = self
                .enumerate_object_fields(sections_obj_idx)
                .into_iter()
                .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                .collect();
            section_names.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
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
                        self.memory_manager.pop_external_roots(); // pop sections_obj_idx root
                        self.memory_manager.pop_external_roots(); // pop obj_idx root
                        return Err(RuntimeError::new(
                            span,
                            format!(
                                "std.manifestIni: section '{}' must be an object, got {}",
                                section_name,
                                other.type_name()
                            ),
                            source_id,
                        ));
                    }
                };
                // Root section_obj_idx for the duration of field processing
                self.memory_manager
                    .push_external_roots(vec![Value::Object(section_obj_idx)], Vec::new());
                let mut section_fields: Vec<(String, Value, Option<ObjectIndex>)> = self
                    .enumerate_object_fields(section_obj_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                section_fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));
                for (key, val, super_obj) in section_fields {
                    let forced_val = match val {
                        Value::Closure(c) => {
                            self.execute_thunk_sync(c, Some(section_obj_idx), super_obj)?
                        }
                        other => other,
                    };
                    let forced_val = self.force_value(forced_val)?;
                    self.ini_field_to_string(
                        &key,
                        forced_val,
                        &mut result,
                        span.clone(),
                        &source_id,
                    )?;
                }
                self.memory_manager.pop_external_roots(); // pop section_obj_idx root
            }
            self.memory_manager.pop_external_roots(); // pop sections_obj_idx root
        }

        self.memory_manager.pop_external_roots(); // pop obj_idx root
        Ok(result)
    }

    fn ini_field_to_string(
        &mut self,
        key: &str,
        value: Value,
        result: &mut String,
        span: Range<usize>,
        source_id: &str,
    ) -> Result<(), RuntimeError> {
        match value {
            Value::Array(arr_idx) => {
                let elements: Vec<Value> = {
                    let arr = self.memory_manager.load_array(arr_idx);
                    arr.elements.clone()
                };
                for elem in elements {
                    let forced_elem = self.force_value(elem)?;
                    let forced_elem = match forced_elem {
                        Value::Closure(c) => {
                            if self.memory_manager.load_closure(c).is_thunk {
                                self.force_thunk(c)?
                            } else {
                                self.execute_thunk_sync(c, None, None)?
                            }
                        }
                        other => other,
                    };
                    let forced_elem = self.force_value(forced_elem)?;
                    let val_str =
                        self.ini_scalar_to_string(forced_elem, span.clone(), source_id)?;
                    result.push_str(&format!("{} = {}\n", key, val_str));
                }
                Ok(())
            }
            _ => {
                let val_str = self.ini_scalar_to_string(value, span, source_id)?;
                result.push_str(&format!("{} = {}\n", key, val_str));
                Ok(())
            }
        }
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
            other => Err(RuntimeError::new(
                span,
                format!(
                    "std.manifestIni: value must be a scalar, got {}",
                    other.type_name()
                ),
                source_id.to_string(),
            )),
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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };

        match forced {
            Value::Null => Ok("None".to_string()),
            Value::Boolean(true) => Ok("True".to_string()),
            Value::Boolean(false) => Ok("False".to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(RuntimeError::new(
                        span,
                        "std.manifestPython: cannot serialize NaN".to_string(),
                        source_id,
                    ));
                }
                if n.is_infinite() {
                    return Err(RuntimeError::new(
                        span,
                        "std.manifestPython: cannot serialize Infinite".to_string(),
                        source_id,
                    ));
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
                    return Ok("[]".to_string());
                }
                let mut items = Vec::with_capacity(elements.len());
                for elem in elements {
                    let s = self.manifest_python_value(
                        elem,
                        depth + 1,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    items.push(s);
                }
                Ok(format!("[{}]", items.join(", ")))
            }
            Value::Object(o_idx) => {
                let mut field_data: Vec<(String, Value, Option<ObjectIndex>)> = self
                    .enumerate_object_fields(o_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                field_data.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

                if field_data.is_empty() {
                    return Ok("{}".to_string());
                }

                // Root the object during field processing
                self.memory_manager
                    .push_external_roots(vec![Value::Object(o_idx)], Vec::new());
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
                    pairs.push(format!("{}: {}", key_out, val_s));
                }
                self.memory_manager.pop_external_roots();
                Ok(format!("{{{}}}", pairs.join(", ")))
            }
            _ => Err(RuntimeError::new(
                span,
                "std.manifestPython: cannot manifest function".to_string(),
                source_id,
            )),
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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "std.manifestPythonVars: expected object, got {}",
                        other.type_name()
                    ),
                    source_id,
                ));
            }
        };

        let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = self
            .enumerate_object_fields(obj_idx)
            .into_iter()
            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
            .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
            .collect();
        fields.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };

        match forced {
            Value::Null => Ok("null".to_string()),
            Value::Boolean(b) => Ok(b.to_string()),
            Value::Number(n) => {
                if n.is_nan() {
                    return Err(RuntimeError::new(
                        span,
                        "std.manifestYamlDoc: cannot serialize NaN".to_string(),
                        source_id,
                    ));
                }
                if n.is_infinite() {
                    return Err(RuntimeError::new(
                        span,
                        "std.manifestYamlDoc: cannot serialize Infinite".to_string(),
                        source_id,
                    ));
                }
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    Ok(format!("{}", n as i64))
                } else {
                    Ok(format!("{}", n))
                }
            }
            Value::String(s_idx) => {
                let s = self.memory_manager.load_string(s_idx).to_string();
                // Use block scalar (|) for multiline strings ending with \n
                if s.contains('\n') && s.ends_with('\n') {
                    // Block scalar: "|" marker + content indented at depth+1
                    let content_indent = "  ".repeat(depth + 1);
                    let content = &s[..s.len() - 1]; // strip trailing \n
                    let indented_lines: Vec<String> = content
                        .split('\n')
                        .map(|line| {
                            if line.is_empty() {
                                String::new()
                            } else {
                                format!("{}{}", content_indent, line)
                            }
                        })
                        .collect();
                    Ok(format!("|\n{}", indented_lines.join("\n")))
                } else if depth == 0 && !yaml_value_needs_quoting(&s) {
                    // Top-level strings that are YAML-safe are bare
                    Ok(s)
                } else {
                    // String values inside structures are double-quoted in Jsonnet YAML output
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
            }
            Value::Array(a_idx) => {
                let elements = self.memory_manager.load_array(a_idx).elements.clone();
                if elements.is_empty() {
                    return Ok("[]".to_string());
                }
                // Root array elements to protect from GC
                self.memory_manager
                    .push_external_roots(elements.clone(), Vec::new());
                let indent = "  ".repeat(depth);
                let mut lines = Vec::with_capacity(elements.len());
                for elem in elements {
                    let forced_elem = self.force_value(elem)?;
                    let forced_elem = match forced_elem {
                        Value::Closure(c) => {
                            if self.memory_manager.load_closure(c).is_thunk {
                                self.force_thunk(c)?
                            } else {
                                self.execute_thunk_sync(c, None, None)?
                            }
                        }
                        other => other,
                    };

                    let is_nonempty_array = if let Value::Array(ai) = forced_elem {
                        !self.memory_manager.load_array(ai).elements.is_empty()
                    } else {
                        false
                    };
                    if is_nonempty_array {
                        // Array element is itself a non-empty array: put "-" alone, indent child below
                        let elem_str = self.manifest_yaml_doc(
                            forced_elem,
                            depth + 1,
                            indent_array_in_object,
                            quote_keys,
                            span.clone(),
                            source_id.clone(),
                        )?;
                        lines.push(format!("{}-\n{}", indent, elem_str));
                    } else {
                        // Non-array element: render at depth+1 and strip child indent
                        // from the first line (the "- " prefix replaces it)
                        let elem_str = self.manifest_yaml_doc(
                            forced_elem,
                            depth + 1,
                            indent_array_in_object,
                            quote_keys,
                            span.clone(),
                            source_id.clone(),
                        )?;
                        let child_indent = "  ".repeat(depth + 1);
                        if elem_str.contains('\n') {
                            let mut sub_lines = elem_str.lines();
                            let first = sub_lines.next().unwrap_or("");
                            let first_stripped = first.strip_prefix(&child_indent).unwrap_or(first);
                            // For block scalars (first line is "|"), strip child indent
                            // from content lines too, since the content indent is absolute
                            // and needs adjusting for the "- " prefix position.
                            let strip_rest = first_stripped == "|";
                            let rest: Vec<String> = sub_lines
                                .map(|l| {
                                    if strip_rest {
                                        l.strip_prefix("  ").unwrap_or(l).to_string()
                                    } else {
                                        l.to_string()
                                    }
                                })
                                .collect();
                            if rest.is_empty() {
                                lines.push(format!("{}- {}", indent, first_stripped));
                            } else {
                                lines.push(format!(
                                    "{}- {}\n{}",
                                    indent,
                                    first_stripped,
                                    rest.join("\n")
                                ));
                            }
                        } else {
                            let stripped =
                                elem_str.strip_prefix(&child_indent).unwrap_or(&elem_str);
                            lines.push(format!("{}- {}", indent, stripped));
                        }
                    }
                }
                self.memory_manager.pop_external_roots();
                Ok(lines.join("\n"))
            }
            Value::Object(o_idx) => {
                // Root the object to protect from GC during field processing
                self.memory_manager
                    .push_external_roots(vec![Value::Object(o_idx)], Vec::new());
                let mut field_data: Vec<(String, Value, Option<ObjectIndex>)> = self
                    .enumerate_object_fields(o_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                field_data.sort_by(|(a, _, _), (b, _, _)| a.cmp(b));

                if field_data.is_empty() {
                    self.memory_manager.pop_external_roots();
                    return Ok("{}".to_string());
                }

                // Root all field values (closures/thunks)
                let field_values: Vec<Value> = field_data.iter().map(|(_, v, _)| *v).collect();
                self.memory_manager
                    .push_external_roots(field_values, Vec::new());

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
                    } else if yaml_needs_quoting(&key_str) {
                        // Even with quote_keys=false, some keys need quoting
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

                    // For arrays in objects, use depth (not depth+1) when
                    // indent_array_in_object is false — array items sit at the
                    // same indent level as the key.
                    let val_is_array = matches!(forced_val, Value::Array(_));
                    let child_depth = if val_is_array && !indent_array_in_object {
                        depth
                    } else {
                        depth + 1
                    };
                    let val_str = self.manifest_yaml_doc(
                        forced_val,
                        child_depth,
                        indent_array_in_object,
                        quote_keys,
                        span.clone(),
                        source_id.clone(),
                    )?;

                    if val_str.starts_with("|\n") {
                        // Block scalar: put "|" inline with key, content below
                        // Strip one level of indent from content lines since the
                        // "|" is now inline with the key (not at child depth)
                        let content_lines: Vec<&str> = val_str[2..].lines().collect();
                        let adjusted: Vec<String> = content_lines
                            .iter()
                            .map(|l| l.strip_prefix("  ").unwrap_or(l).to_string())
                            .collect();
                        lines.push(format!(
                            "{}{}: |\n{}",
                            indent,
                            key_repr,
                            adjusted.join("\n")
                        ));
                    } else if val_str.contains('\n') {
                        // Multi-line value: key on its own line, value already indented
                        lines.push(format!("{}{}:\n{}", indent, key_repr, val_str));
                    } else if val_is_array && val_str != "[]" {
                        // Non-empty array value: put on next line for YAML style
                        lines.push(format!("{}{}:\n{}", indent, key_repr, val_str));
                    } else {
                        lines.push(format!("{}{}: {}", indent, key_repr, val_str));
                    }
                }
                self.memory_manager.pop_external_roots(); // field values
                self.memory_manager.pop_external_roots(); // object
                Ok(lines.join("\n"))
            }
            other => Err(RuntimeError::new(
                span,
                format!("std.manifestYamlDoc: cannot manifest {}", other.type_name()),
                source_id,
            )),
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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };
        let arr_idx = match forced {
            Value::Array(idx) => idx,
            other => {
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "manifestYamlStream: expected array, got {}",
                        other.type_name()
                    ),
                    source_id,
                ));
            }
        };

        let elements = self.memory_manager.load_array(arr_idx).elements.clone();

        if elements.is_empty() {
            return Ok(String::new());
        }

        // Root elements to protect from GC
        self.memory_manager
            .push_external_roots(elements.clone(), Vec::new());

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

        self.memory_manager.pop_external_roots();
        let mut result = parts.join("\n");
        if c_document_end {
            result.push_str("\n...\n");
        } else {
            result.push('\n');
        }
        Ok(result)
    }

    fn parse_yaml_multi_doc(
        &mut self,
        s: &str,
        span: Range<usize>,
        source_id: String,
    ) -> Result<Value, RuntimeError> {
        // Split YAML into documents by --- separators
        // A line matching /^---(\s|$)/ separates documents
        let mut doc_strings: Vec<String> = Vec::new();
        let mut current_doc = String::new();
        let mut has_separator = false;
        let mut seen_content_before_separator = false;

        for line in s.lines() {
            let trimmed = line.trim_end();
            if trimmed == "---" || trimmed.starts_with("--- ") {
                has_separator = true;
                if seen_content_before_separator || !current_doc.trim().is_empty() {
                    doc_strings.push(current_doc.clone());
                    seen_content_before_separator = true;
                }
                current_doc.clear();
            } else {
                if !current_doc.is_empty() {
                    current_doc.push('\n');
                }
                current_doc.push_str(line);
                if !line.trim().is_empty() {
                    seen_content_before_separator = true;
                }
            }
        }
        // Push the last document
        doc_strings.push(current_doc);

        if !has_separator {
            // Single document - parse normally
            let yaml_val: serde_yaml::Value = serde_yaml::from_str(s).map_err(|e| {
                RuntimeError::new(span.clone(), format!("parseYaml: {}", e), source_id.clone())
            })?;
            return self.serde_yaml_to_jsonnet_value(yaml_val, span, source_id);
        }

        // Multiple documents → parse each and return array
        let mut elements = Vec::new();
        for doc_str in &doc_strings {
            let trimmed = doc_str.trim();
            if trimmed.is_empty() {
                elements.push(Value::Null);
            } else {
                let yaml_val: serde_yaml::Value = serde_yaml::from_str(trimmed).map_err(|e| {
                    RuntimeError::new(span.clone(), format!("parseYaml: {}", e), source_id.clone())
                })?;
                let val =
                    self.serde_yaml_to_jsonnet_value(yaml_val, span.clone(), source_id.clone())?;
                elements.push(val);
            }
        }
        let alloc = self.memory_manager.allocate_array(elements);
        Ok(Value::Array(alloc.index))
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
                    Err(RuntimeError::new(
                        span,
                        "parseYaml: unsupported number".to_string(),
                        source_id,
                    ))
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
                    props.insert(key_idx, ObjectField::new(val, FieldVisibility::Visible));
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
        self.memory_manager.external_roots.push(vec![value]);
        let result = self.manifest_xml_jsonml_inner(value, span, source_id);
        self.memory_manager.external_roots.pop();
        result
    }

    fn manifest_xml_jsonml_inner(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        let forced = self.force_value(value)?;
        let forced = match forced {
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };
        match forced {
            Value::String(idx) => {
                let s = self.memory_manager.load_string(idx).to_string();
                Ok(xml_escape(&s))
            }
            Value::Array(arr_idx) => {
                // Force all array elements first to avoid GC issues during recursive processing
                self.force_all_array_elements(arr_idx)?;
                let elements = self.memory_manager.load_array(arr_idx).elements.clone();
                // Root the elements to protect from GC during recursive calls
                self.memory_manager.external_roots.push(elements.clone());
                if elements.is_empty() {
                    self.memory_manager.external_roots.pop();
                    return Err(RuntimeError::new(
                        span,
                        "manifestXmlJsonml: array must have at least one element (tag name)"
                            .to_string(),
                        source_id,
                    ));
                }
                // First element: tag name
                let tag_val = self.force_value(elements[0])?;
                let tag_val = match tag_val {
                    Value::Closure(c) => {
                        if self.memory_manager.load_closure(c).is_thunk {
                            self.force_thunk(c)?
                        } else {
                            self.execute_thunk_sync(c, None, None)?
                        }
                    }
                    other => other,
                };
                let tag = match tag_val {
                    Value::String(idx) => self.memory_manager.load_string(idx).to_string(),
                    other => {
                        return Err(RuntimeError::new(
                            span,
                            format!(
                                "manifestXmlJsonml: first element must be string tag, got {}",
                                other.type_name()
                            ),
                            source_id,
                        ));
                    }
                };

                let mut attrs = String::new();
                let mut child_start = 1;

                // Check if second element is an attribute object
                if elements.len() > 1 {
                    let second_val = self.force_value(elements[1])?;
                    let second_val = match second_val {
                        Value::Closure(c) => {
                            if self.memory_manager.load_closure(c).is_thunk {
                                self.force_thunk(c)?
                            } else {
                                self.execute_thunk_sync(c, None, None)?
                            }
                        }
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
                                Value::Closure(c) => {
                                    self.execute_thunk_sync(c, Some(obj_idx), None)?
                                }
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
                                    return Err(RuntimeError::new(
                                        span.clone(),
                                        format!(
                                            "manifestXmlJsonml: attribute value must be scalar, got {}",
                                            other.type_name()
                                        ),
                                        source_id.clone(),
                                    ));
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

                self.memory_manager.external_roots.pop();
                Ok(format!("<{}{}>{}</{}>", tag, attrs, children, tag))
            }
            other => Err(RuntimeError::new(
                span,
                format!(
                    "manifestXmlJsonml: expected string or array, got {}",
                    other.type_name()
                ),
                source_id,
            )),
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
            Value::Closure(c) => {
                if self.memory_manager.load_closure(c).is_thunk {
                    self.force_thunk(c)?
                } else {
                    self.execute_thunk_sync(c, None, None)?
                }
            }
            other => other,
        };
        let obj_idx = match forced {
            Value::Object(idx) => idx,
            other => {
                return Err(RuntimeError::new(
                    span,
                    format!("manifestTomlEx: expected object, got {}", other.type_name()),
                    source_id,
                ));
            }
        };
        let indent_owned = indent.to_string();
        let result = self.manifest_toml_table(obj_idx, &indent_owned, &[], 0, span, source_id)?;
        // Trim trailing newline - caller adds it back if needed
        Ok(result.trim_end_matches('\n').to_string())
    }

    fn manifest_toml_table(
        &mut self,
        obj_idx: ObjectIndex,
        indent: &str,
        path: &[String],
        depth: usize,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        // Root the object to protect from GC during recursive processing
        self.memory_manager
            .external_roots
            .push(vec![Value::Object(obj_idx)]);
        let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = self
            .enumerate_object_fields(obj_idx)
            .into_iter()
            .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
            .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        // Root all field values to protect from GC during recursive processing
        let field_values: Vec<Value> = fields.iter().map(|(_, v, _)| *v).collect();
        self.memory_manager.external_roots.push(field_values);

        let content_indent = indent.repeat(depth);

        let mut scalars = String::new();
        // Collect sub-sections: (key, is_array_of_tables, forced_data)
        // We'll store the data needed to render each sub-section later
        enum SubSection {
            Table {
                key: String,
                sub_obj_idx: ObjectIndex,
            },
            ArrayOfTables {
                key: String,
                elem_objs: Vec<ObjectIndex>,
            },
        }
        let mut sub_sections: Vec<SubSection> = Vec::new();
        // Pre-push an empty vec for sub-section roots so we can add to it during the loop
        self.memory_manager.external_roots.push(Vec::new());
        let sub_section_roots_idx = self.memory_manager.external_roots.len() - 1;

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
                    // Root immediately to protect from GC (use index, not last_mut)
                    self.memory_manager.external_roots[sub_section_roots_idx]
                        .push(Value::Object(sub_obj_idx));
                    sub_sections.push(SubSection::Table {
                        key: key.clone(),
                        sub_obj_idx,
                    });
                }
                Value::Array(arr_idx) => {
                    self.force_all_array_elements(arr_idx)?;
                    let elems = self.memory_manager.load_array(arr_idx).elements.clone();
                    self.memory_manager.external_roots.push(elems.clone());
                    let is_array_of_objects = if elems.is_empty() {
                        false
                    } else {
                        let first_forced = self.force_value(elems[0])?;
                        let first_forced = match first_forced {
                            Value::Closure(c) => {
                                if self.memory_manager.load_closure(c).is_thunk {
                                    self.force_thunk(c)?
                                } else {
                                    self.execute_thunk_sync(c, None, None)?
                                }
                            }
                            other => other,
                        };
                        matches!(first_forced, Value::Object(_))
                    };
                    if is_array_of_objects {
                        // Collect all object indices
                        let mut elem_objs = Vec::new();
                        for elem in &elems {
                            let elem_forced = self.force_value(*elem)?;
                            let elem_forced = match elem_forced {
                                Value::Closure(c) => {
                                    if self.memory_manager.load_closure(c).is_thunk {
                                        self.force_thunk(c)?
                                    } else {
                                        self.execute_thunk_sync(c, None, None)?
                                    }
                                }
                                other => other,
                            };
                            let sub_obj_idx = match elem_forced {
                                Value::Object(idx) => idx,
                                other => {
                                    self.memory_manager.external_roots.pop(); // elems
                                    self.memory_manager.external_roots.pop(); // sub-section roots
                                    self.memory_manager.external_roots.pop(); // field values
                                    self.memory_manager.external_roots.pop(); // object
                                    return Err(RuntimeError::new(
                                        span.clone(),
                                        format!(
                                            "manifestTomlEx: mixed arrays not supported, got {}",
                                            other.type_name()
                                        ),
                                        source_id.clone(),
                                    ));
                                }
                            };
                            // Root immediately using stable index (not last_mut which points to elems)
                            self.memory_manager.external_roots[sub_section_roots_idx]
                                .push(Value::Object(sub_obj_idx));
                            elem_objs.push(sub_obj_idx);
                        }
                        sub_sections.push(SubSection::ArrayOfTables {
                            key: key.clone(),
                            elem_objs,
                        });
                    } else {
                        // Render as inline or multiline array for scalars
                        let escaped_key = toml_escape_key(&key);
                        if elems.is_empty() {
                            scalars.push_str(&format!("{}{} = []\n", content_indent, escaped_key));
                        } else {
                            // Multiline array
                            let inner_indent = indent.repeat(depth + 1);
                            let mut arr_str = format!("{}{} = [\n", content_indent, escaped_key);
                            for (i, elem) in elems.iter().enumerate() {
                                let elem_forced = self.force_value(*elem)?;
                                let elem_forced = match elem_forced {
                                    Value::Closure(c) => {
                                        if self.memory_manager.load_closure(c).is_thunk {
                                            self.force_thunk(c)?
                                        } else {
                                            self.execute_thunk_sync(c, None, None)?
                                        }
                                    }
                                    other => other,
                                };
                                let val_str = self.manifest_toml_inline_value(
                                    elem_forced,
                                    span.clone(),
                                    source_id.clone(),
                                )?;
                                if i < elems.len() - 1 {
                                    arr_str.push_str(&format!("{}{},\n", inner_indent, val_str));
                                } else {
                                    arr_str.push_str(&format!("{}{}\n", inner_indent, val_str));
                                }
                            }
                            arr_str.push_str(&format!("{}]\n", content_indent));
                            scalars.push_str(&arr_str);
                        }
                    }
                    self.memory_manager.external_roots.pop(); // elems
                }
                scalar => {
                    let escaped_key = toml_escape_key(&key);
                    let val_str =
                        self.manifest_toml_scalar(scalar, span.clone(), source_id.clone())?;
                    scalars.push_str(&format!(
                        "{}{} = {}\n",
                        content_indent, escaped_key, val_str
                    ));
                }
            }
        }

        // Now render sub-sections in alphabetical order (they are already sorted by key)
        let mut sections_str = String::new();
        for sub_section in &sub_sections {
            match sub_section {
                SubSection::Table { key, sub_obj_idx } => {
                    let mut sub_path = path.to_vec();
                    sub_path.push(key.clone());
                    let path_str = toml_format_path(&sub_path);
                    let indent_owned = indent.to_string();
                    let sub_content = self.manifest_toml_table(
                        *sub_obj_idx,
                        &indent_owned,
                        &sub_path,
                        depth + 1,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    sections_str.push_str(&format!(
                        "\n{}[{}]\n{}",
                        content_indent, path_str, sub_content
                    ));
                }
                SubSection::ArrayOfTables { key, elem_objs } => {
                    let mut sub_path = path.to_vec();
                    sub_path.push(key.clone());
                    let path_str = toml_format_path(&sub_path);
                    for sub_obj_idx in elem_objs {
                        let indent_owned = indent.to_string();
                        let sub_content = self.manifest_toml_table(
                            *sub_obj_idx,
                            &indent_owned,
                            &sub_path,
                            depth + 1,
                            span.clone(),
                            source_id.clone(),
                        )?;
                        sections_str.push_str(&format!(
                            "\n{}[[{}]]\n{}",
                            content_indent, path_str, sub_content
                        ));
                    }
                }
            }
        }

        self.memory_manager.external_roots.pop(); // sub-section roots
        self.memory_manager.external_roots.pop(); // field values
        self.memory_manager.external_roots.pop(); // object
        Ok(format!("{}{}", scalars, sections_str))
    }

    fn manifest_toml_scalar(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::Null => Err(RuntimeError::new(
                span,
                "manifestTomlEx: null values are not supported in TOML".to_string(),
                source_id,
            )),
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
            other => Err(RuntimeError::new(
                span,
                format!(
                    "manifestTomlEx: unexpected value type {}",
                    other.type_name()
                ),
                source_id,
            )),
        }
    }

    fn manifest_toml_inline_array(
        &mut self,
        arr_idx: chunk::ArrayIndex,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        self.force_all_array_elements(arr_idx)?;
        let elems = self.memory_manager.load_array(arr_idx).elements.clone();
        self.memory_manager.external_roots.push(elems.clone());
        if elems.is_empty() {
            self.memory_manager.external_roots.pop();
            return Ok("[]".to_string());
        }
        let mut parts = Vec::new();
        for elem in elems {
            let forced = self.force_value(elem)?;
            let forced = match forced {
                Value::Closure(c) => {
                    if self.memory_manager.load_closure(c).is_thunk {
                        self.force_thunk(c)?
                    } else {
                        self.execute_thunk_sync(c, None, None)?
                    }
                }
                other => other,
            };
            let s = self.manifest_toml_inline_value(forced, span.clone(), source_id.clone())?;
            parts.push(s);
        }
        self.memory_manager.external_roots.pop();
        Ok(format!("[ {} ]", parts.join(", ")))
    }

    /// Format a value as an inline TOML value (for use in inline arrays/tables).
    fn manifest_toml_inline_value(
        &mut self,
        value: Value,
        span: Range<usize>,
        source_id: String,
    ) -> Result<String, RuntimeError> {
        match value {
            Value::Array(arr_idx) => {
                // Render as inline TOML array: [ elem1, elem2 ]
                self.manifest_toml_inline_array(arr_idx, span, source_id)
            }
            Value::Object(obj_idx) => {
                // Render as inline TOML table: { key = val, key2 = val2 }
                self.memory_manager
                    .push_external_roots(vec![Value::Object(obj_idx)], Vec::new());
                let mut fields: Vec<(String, Value, Option<ObjectIndex>)> = self
                    .enumerate_object_fields(obj_idx)
                    .into_iter()
                    .filter(|(_, _, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, so, _)| (self.memory_manager.load_string(k).to_string(), v, so))
                    .collect();
                fields.sort_by(|a, b| a.0.cmp(&b.0));
                let mut pairs = Vec::new();
                for (key, val, super_obj) in fields {
                    let forced_val = match val {
                        Value::Closure(c) => {
                            self.execute_thunk_sync(c, Some(obj_idx), super_obj)?
                        }
                        other => self.force_value(other)?,
                    };
                    let forced_val = match forced_val {
                        Value::Closure(c) => {
                            self.execute_thunk_sync(c, Some(obj_idx), super_obj)?
                        }
                        other => other,
                    };
                    let val_str = self.manifest_toml_inline_value(
                        forced_val,
                        span.clone(),
                        source_id.clone(),
                    )?;
                    let escaped_key = toml_escape_key(&key);
                    pairs.push(format!("{} = {}", escaped_key, val_str));
                }
                self.memory_manager.pop_external_roots();
                Ok(format!("{{ {} }}", pairs.join(", ")))
            }
            _ => self.manifest_toml_scalar(value, span, source_id),
        }
    }
}

/// Check if a TOML key needs quoting. Bare keys may only contain ASCII letters,
/// digits, dashes, and underscores.
fn toml_key_needs_quoting(key: &str) -> bool {
    if key.is_empty() {
        return true;
    }
    !key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Escape a TOML key: if it contains special characters, wrap in quotes with escaping.
fn toml_escape_key(key: &str) -> String {
    if toml_key_needs_quoting(key) {
        let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        key.to_string()
    }
}

/// Format a TOML section path like `section."e$caped".nested`
fn toml_format_path(path: &[String]) -> String {
    path.iter()
        .map(|p| toml_escape_key(p))
        .collect::<Vec<_>>()
        .join(".")
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

    let bytes = s.as_bytes();

    // Contains whitespace (space or tab) → quote
    if s.contains(' ') || s.contains('\t') {
        return true;
    }

    // Contains '#' → quote (YAML comment indicator)
    if s.contains('#') {
        return true;
    }

    // Starts with a YAML indicator character → quote
    let first = bytes[0];
    if b"{[],&*?|<>=!%@`'\"~+.".contains(&first) {
        return true;
    }

    // Is exactly '-' or '---', or starts with '- ' (dash-space, caught by space check)
    if s == "-" || s == "---" {
        return true;
    }

    // YAML boolean keywords (case-insensitive)
    let lower = s.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n"
    ) {
        return true;
    }

    // YAML null (case-insensitive)
    if lower == "null" {
        return true;
    }

    // Starts with '-' followed by '.' or digit → check if the remainder is a YAML number.
    // E.g. "-1_0" and "-.inf" need quoting, but "-0B1010..." (uppercase B) does not.
    if first == b'-' && bytes.len() > 1 && (bytes[1] == b'.' || bytes[1].is_ascii_digit()) {
        return yaml_looks_like_number(&s[1..]);
    }

    // Starts with a digit → check if YAML would interpret as number or timestamp
    if first.is_ascii_digit() {
        return yaml_looks_like_number(s);
    }

    false
}

/// Check if a YAML string value needs quoting (for top-level document values).
/// This is more comprehensive than key quoting since values can contain more special chars.
fn yaml_value_needs_quoting(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    // Contains newlines
    if s.contains('\n') || s.contains('\r') {
        return true;
    }
    // Contains colon-space or trailing colon
    if s.contains(": ") || s.ends_with(':') {
        return true;
    }
    // Use the key quoting logic for the rest (booleans, numbers, special chars, etc.)
    yaml_needs_quoting(s)
}

/// Check if a string (starting with a digit or '.') looks like a YAML 1.1 numeric
/// or timestamp value. Called on the portion after an optional leading '-'.
fn yaml_looks_like_number(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }

    // Starts with '.' → .inf, .NaN, or float like .5 — already handled by indicator check
    // in the caller, but if we get here after stripping '-', check for -.inf etc.
    if bytes[0] == b'.' {
        // After stripping '-', we have ".inf", ".NaN", ".5" etc. → quote
        return true;
    }

    // Must start with a digit at this point
    if !bytes[0].is_ascii_digit() {
        return false;
    }

    // Hex literal: 0x (lowercase x only, YAML 1.1)
    if bytes.len() > 1 && bytes[0] == b'0' && bytes[1] == b'x' {
        return true;
    }

    // Binary literal: 0b (lowercase b only, YAML 1.1)
    if bytes.len() > 1 && bytes[0] == b'0' && bytes[1] == b'b' {
        return true;
    }

    // Octal: starts with 0 followed by only digits and underscores
    if bytes[0] == b'0'
        && bytes.len() > 1
        && bytes[1..].iter().all(|&b| b.is_ascii_digit() || b == b'_')
    {
        return true;
    }

    // Contains ':' → sexagesimal or timestamp → quote
    if s.contains(':') {
        return true;
    }

    let dot_count = s.chars().filter(|&c| c == '.').count();

    // Multiple dots like 192.168.0.1 → not a YAML number → safe
    if dot_count > 1 {
        return false;
    }

    // Count dashes in the string (excluding those in scientific notation exponent)
    // A dash is "in exponent" if preceded by 'e' or 'E'
    let has_non_exponent_dash = s.char_indices().any(|(i, c)| {
        c == '-' && (i == 0 || !matches!(s.as_bytes().get(i - 1), Some(b'e') | Some(b'E')))
    });

    if has_non_exponent_dash {
        // Has dashes not in scientific notation position.
        // Check if it's a timestamp pattern (4 digits then dash)
        if bytes.len() >= 5 && bytes[0..4].iter().all(|b| b.is_ascii_digit()) && bytes[4] == b'-' {
            return true;
        }
        // Otherwise dashes make it not a YAML number (e.g. 1-234-567-8901)
        return false;
    }

    // Check if all chars are in the YAML number character set
    // (digits, underscores, dot, e, E, +, -) which would make it a YAML number
    let is_yaml_number = s.chars().all(|c| {
        c.is_ascii_digit() || c == '_' || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-'
    });
    if is_yaml_number {
        return true;
    }

    false
}

/// Main execution function - entry point for running Jsonnet bytecode
pub fn execute(
    chunk: Chunk,
    memory_manager: MemoryManager,
) -> Result<serde_json::Value, RuntimeError> {
    execute_with_ext_vars(chunk, memory_manager, &[], &[], &[])
}

/// Execute with external variables set via --ext-str / --ext-code CLI flags
pub fn execute_with_ext_vars(
    chunk: Chunk,
    memory_manager: MemoryManager,
    ext_strs: &[(String, String)],
    ext_codes: &[(String, String)],
    jpaths: &[String],
) -> Result<serde_json::Value, RuntimeError> {
    let mut vm = VirtualMachine::new(chunk, memory_manager);
    vm.set_jpaths(jpaths.to_vec());

    for (k, v) in ext_strs {
        vm.set_ext_var_string(k, v);
    }

    // Compile and set ext-code vars
    for (k, code) in ext_codes {
        vm.set_ext_var_code(k, code)?;
    }

    let mut value = vm.interpret()?;

    // Auto-invoke top-level function if it has 0 required parameters
    if let Value::Closure(ci) = value {
        let func = vm
            .memory_manager
            .load_function(vm.memory_manager.load_closure(ci).function);
        if func.required_params == 0 {
            value = vm.call_test_closure(ci)?;
        }
    }

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

    #[test]
    fn test_coverage_collector_records_spans() {
        let source = "1 + 2";
        let mut scanner = scanner::Scanner::new(source, "test_coverage");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test_coverage");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        vm.enable_coverage();
        let _ = vm.interpret();
        let collector = vm.take_coverage().unwrap();

        assert!(collector.total_spans_hit() > 0);
        assert!(collector.spans_for_source("test_coverage").is_some());
    }

    #[test]
    fn test_coverage_disabled_by_default() {
        let source = "1 + 2";
        let mut scanner = scanner::Scanner::new(source, "test_no_cov");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test_no_cov");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let _ = vm.interpret();
        assert!(vm.take_coverage().is_none());
    }

    #[test]
    fn test_force_field_thunk() {
        let source = r#"{ x: 42 }"#;
        let mut scanner = scanner::Scanner::new(source, "test_force");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test_force");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        if let Value::Object(obj_idx) = result {
            let (thunk_ci, super_obj) = {
                let obj = vm.memory_manager().load_object(obj_idx);
                let field = obj.properties.values().next().unwrap();
                match field.value {
                    Value::Closure(ci) => (ci, obj.base_object),
                    _ => panic!("Expected thunk closure"),
                }
            };
            let val = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
            if let Value::Number(n) = val {
                assert_eq!(n, 42.0);
            } else {
                panic!("Expected number, got {:?}", val);
            }
        } else {
            panic!("Expected object");
        }
    }

    #[test]
    fn test_call_test_closure_pass() {
        let source = r#"{ testPass(): std.assertEqual(1 + 1, 2) }"#;
        let mut scanner = scanner::Scanner::new(source, "test_call");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test_call");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        if let Value::Object(obj_idx) = result {
            let (thunk_ci, super_obj) = {
                let obj = vm.memory_manager().load_object(obj_idx);
                let mut found = None;
                for (key, field) in &obj.properties {
                    let name = vm.memory_manager().load_string(*key);
                    if name == "testPass" {
                        if let Value::Closure(ci) = field.value {
                            found = Some((ci, obj.base_object));
                        }
                    }
                }
                found.expect("testPass should be a thunk")
            };
            let forced = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
            if let Value::Closure(func_ci) = forced {
                let result = vm.call_test_closure(func_ci);
                assert!(result.is_ok(), "testPass should succeed");
            } else {
                panic!("Expected closure after forcing thunk");
            }
        } else {
            panic!("Expected object result");
        }
    }

    #[test]
    fn test_call_test_closure_fail() {
        let source = r#"{ testFail(): std.assertEqual(1, 2) }"#;
        let mut scanner = scanner::Scanner::new(source, "test_call_fail");
        let mut memory_manager = MemoryManager::new();
        let compiler = compiler::Compiler::new(&mut scanner, "test_call_fail");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        let mut vm = VirtualMachine::new(chunk, memory_manager);
        let result = vm.interpret().unwrap();

        if let Value::Object(obj_idx) = result {
            let (thunk_ci, super_obj) = {
                let obj = vm.memory_manager().load_object(obj_idx);
                let mut found = None;
                for (key, field) in &obj.properties {
                    let name = vm.memory_manager().load_string(*key);
                    if name == "testFail" {
                        if let Value::Closure(ci) = field.value {
                            found = Some((ci, obj.base_object));
                        }
                    }
                }
                found.expect("testFail should be a thunk")
            };
            let forced = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
            if let Value::Closure(func_ci) = forced {
                let result = vm.call_test_closure(func_ci);
                assert!(result.is_err(), "testFail should return an error");
            } else {
                panic!("Expected closure after forcing thunk");
            }
        } else {
            panic!("Expected object result");
        }
    }

    #[test]
    fn test_force_value_thunk_materialization() {
        let mut chunk = create_test_chunk();
        // Create a thunk that returns 42
        let idx_42 = chunk.add_constant(Value::Number(42.0));
        chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 0..5);
        chunk.write_opcode(Opcode::Return, 5..10);

        let mut mm = MemoryManager::new();
        let func_idx = mm.allocate_function(None, 0, 0, chunk.into_owned()).index;
        let thunk_idx = mm.allocate_thunk(func_idx, vec![]).index;

        let mut vm = VirtualMachine::new(create_test_chunk(), mm);
        let result = vm.force_value(Value::Closure(thunk_idx)).unwrap();

        assert_eq!(result, Value::Number(42.0));
    }
}
