use chunk::{
    ArrayIndex, BinaryIndex, ClosureIndex, FunctionIndex, ImportIndex, ObjectIndex, OwnedChunk,
    SpanRunLength, StringIndex, UpvalueIndex, Value,
};
use slotmap::SlotMap;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq)]
pub struct AllocationResult<T> {
    pub should_garbage_collect: bool,
    pub index: T,
}

/// A Jsonnet object containing property key-value pairs
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedObject {
    /// Object properties mapping interned string keys to values
    pub properties: HashMap<StringIndex, Value>,
    // GC marking
    marked: Cell<bool>,
}

impl ManagedObject {
    /// Calculate the actual size of this object including HashMap overhead
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        // HashMap capacity accounts for actual allocated memory, not just length
        let map_capacity_bytes = self.properties.capacity()
            * (std::mem::size_of::<StringIndex>() + std::mem::size_of::<Value>());
        base_size + map_capacity_bytes
    }

    /// Create a new empty Jsonnet object
    pub fn new() -> Self {
        let properties = HashMap::new();
        Self {
            properties,
            marked: Cell::new(false),
        }
    }

    /// Create a Jsonnet object with the given properties
    pub fn with_properties(properties: HashMap<StringIndex, Value>) -> Self {
        Self {
            properties,
            marked: Cell::new(false),
        }
    }

    /// Get a property value by key
    pub fn get(&self, key: &StringIndex) -> Option<&Value> {
        self.properties.get(key)
    }

    /// Check if object has a property
    pub fn has_property(&self, key: &StringIndex) -> bool {
        self.properties.contains_key(key)
    }

    /// Get number of properties
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Check if object is empty
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}

#[derive(Debug, Clone, Eq)]
pub struct ManagedString {
    pub content: String,
    marked: Cell<bool>,
}

impl PartialEq for ManagedString {
    fn eq(&self, other: &ManagedString) -> bool {
        other.content == self.content
    }
}

impl Hash for ManagedString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.content.hash(state);
    }
}

impl ManagedString {
    fn new(content: String) -> Self {
        Self {
            content,
            marked: Cell::new(false),
        }
    }
    fn size(&self) -> usize {
        self.content.len() + std::mem::size_of::<Self>()
    }
}

impl std::fmt::Display for ManagedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.content.as_str())
    }
}

/// A Jsonnet array containing a vector of values
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedArray {
    /// Array elements
    pub elements: Vec<Value>,
    // GC marking
    marked: Cell<bool>,
}

impl ManagedArray {
    /// Calculate the actual size of this array including Vec overhead
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        // Vec capacity accounts for actual allocated memory
        let vec_capacity_bytes = self.elements.capacity() * std::mem::size_of::<Value>();
        base_size + vec_capacity_bytes
    }

    /// Create a new Jsonnet array with the given elements
    pub fn new(elements: Vec<Value>) -> Self {
        Self {
            elements,
            marked: Cell::new(false),
        }
    }

    /// Get the number of elements
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if array is empty
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

/// A heap-allocated function object with its bytecode and metadata
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedFunction {
    /// Optional function name for debugging and error reporting
    pub name: Option<StringIndex>,
    /// Number of parameters this function accepts
    pub arity: u8,
    /// Number of upvalues this function closes over
    pub upvalue_count: u8,
    /// The bytecode chunk containing the function's instructions
    pub chunk: OwnedChunk,
    /// GC marking flag
    marked: Cell<bool>,
}

impl ManagedFunction {
    /// Create a new function object
    pub fn new(name: Option<StringIndex>, arity: u8, upvalue_count: u8, chunk: OwnedChunk) -> Self {
        Self {
            name,
            arity,
            upvalue_count,
            chunk,
            marked: Cell::new(false),
        }
    }

    /// Calculate the actual size of this function including its chunk
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        // Add the size of the owned chunk's allocated memory
        let chunk_size = self.chunk.code.capacity()
            + (self.chunk.spans.capacity() * std::mem::size_of::<SpanRunLength>())
            + (self.chunk.constants.capacity() * std::mem::size_of::<Value>())
            + self.chunk.source_id.capacity();
        base_size + chunk_size
    }
}

/// A heap-allocated upvalue for capturing variables in closures
/// Upvalues can be "open" (pointing to a stack location) or "closed" (holding a captured value)
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedUpvalue {
    /// Stack location when upvalue is open, None when closed
    pub stack_location: Option<usize>,
    /// Captured value when upvalue is closed, None when open
    pub closed_value: Option<Value>,
    /// Pointer to next upvalue in linked list of open upvalues
    pub next: Option<UpvalueIndex>,
    /// GC marking flag
    marked: Cell<bool>,
}

impl ManagedUpvalue {
    /// Create a new open upvalue pointing to a stack location
    pub fn new_open(stack_location: usize) -> Self {
        Self {
            stack_location: Some(stack_location),
            closed_value: None,
            next: None,
            marked: Cell::new(false),
        }
    }

    /// Create a new closed upvalue with a captured value
    pub fn new_closed(value: Value) -> Self {
        Self {
            stack_location: None,
            closed_value: Some(value),
            next: None,
            marked: Cell::new(false),
        }
    }

    /// Close this upvalue by capturing the value from the stack
    pub fn close(&mut self, value: Value) {
        self.stack_location = None;
        self.closed_value = Some(value);
    }

    /// Check if this upvalue is closed
    pub fn is_closed(&self) -> bool {
        self.stack_location.is_none()
    }

    /// Calculate the size of this upvalue
    fn size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A heap-allocated closure combining a function with its captured upvalues
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedClosure {
    /// The function this closure wraps
    pub function: FunctionIndex,
    /// Captured upvalues for this closure
    pub upvalues: Vec<UpvalueIndex>,
    /// GC marking flag
    marked: Cell<bool>,
}

impl ManagedClosure {
    /// Create a new closure wrapping a function with its upvalues
    pub fn new(function: FunctionIndex, upvalues: Vec<UpvalueIndex>) -> Self {
        Self {
            function,
            upvalues,
            marked: Cell::new(false),
        }
    }

    /// Calculate the size of this closure including its upvalue vector
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        let upvalues_size = self.upvalues.capacity() * std::mem::size_of::<UpvalueIndex>();
        base_size + upvalues_size
    }
}

/// A heap-allocated representation of an imported file
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedImport {
    /// String index for the file path
    pub path: StringIndex,
    /// Cached evaluation result, if any
    pub cached_result: Option<Value>,
    /// Whether this import is currently being evaluated (for cycle detection)
    pub evaluating: Cell<bool>,
    /// GC marking flag
    marked: Cell<bool>,
}

impl ManagedImport {
    pub fn new(path: StringIndex) -> Self {
        Self {
            path,
            cached_result: None,
            evaluating: Cell::new(false),
            marked: Cell::new(false),
        }
    }

    fn size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

/// A heap-allocated binary object containing raw bytes
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedBinary {
    /// The raw bytes
    pub data: Vec<u8>,
    /// GC marking flag
    marked: Cell<bool>,
}

impl ManagedBinary {
    /// Create a new binary object
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            data,
            marked: Cell::new(false),
        }
    }

    /// Calculate the actual size of this binary including its data
    fn size(&self) -> usize {
        std::mem::size_of::<Self>() + self.data.capacity()
    }
}

/// A HashSet-based string interning system with garbage collection support
pub struct MemoryManager {
    /// HashSet containing all interned strings
    interned_strings: HashMap<String, StringIndex>,
    strings: SlotMap<StringIndex, ManagedString>,
    /// Collection of Objects
    objects: SlotMap<ObjectIndex, ManagedObject>,
    /// Collection of Arrays
    arrays: SlotMap<ArrayIndex, ManagedArray>,
    /// Collection of Functions
    functions: SlotMap<FunctionIndex, ManagedFunction>,
    /// Collection of Closures
    closures: SlotMap<ClosureIndex, ManagedClosure>,
    /// Collection of Upvalues
    upvalues: SlotMap<UpvalueIndex, ManagedUpvalue>,
    /// Collection of Imports
    imports: SlotMap<ImportIndex, ManagedImport>,
    /// Collection of Binary objects
    binaries: SlotMap<BinaryIndex, ManagedBinary>,
    /// Lookup map for imports by absolute path to ensure uniqueness
    import_lookup: HashMap<String, ImportIndex>,
    /// Total bytes allocated for all strings
    allocated_bytes: usize,
    /// GC threshold for triggering collection
    gc_threshold: usize,
    /// External roots to be considered during GC (used for nested VM execution)
    external_roots: Vec<Vec<Value>>,
    /// External upvalue roots to be considered during GC
    external_upvalue_roots: Vec<Vec<UpvalueIndex>>,
}

impl MemoryManager {
    /// Create a new string interning set
    pub fn new() -> Self {
        Self {
            interned_strings: HashMap::new(),
            strings: SlotMap::new(),
            objects: SlotMap::new(),
            arrays: SlotMap::new(),
            functions: SlotMap::with_key(),
            closures: SlotMap::with_key(),
            upvalues: SlotMap::with_key(),
            imports: SlotMap::with_key(),
            binaries: SlotMap::with_key(),
            import_lookup: HashMap::new(),
            allocated_bytes: 0,
            gc_threshold: 1024 * 1024, // 1MB initial threshold
            external_roots: Vec::new(),
            external_upvalue_roots: Vec::new(),
        }
    }

    /// Push a set of external roots to be protected from GC
    pub fn push_external_roots(&mut self, roots: Vec<Value>, upvalues: Vec<UpvalueIndex>) {
        self.external_roots.push(roots);
        self.external_upvalue_roots.push(upvalues);
    }

    /// Pop the last set of external roots
    pub fn pop_external_roots(&mut self) {
        self.external_roots.pop();
        self.external_upvalue_roots.pop();
    }

    /// Allocate_String a string, returning a handle
    pub fn allocate_string(&mut self, content: &str) -> AllocationResult<StringIndex> {
        if let Some(index) = self.interned_strings.get(content) {
            AllocationResult {
                index: *index,
                should_garbage_collect: false,
            }
        } else {
            let managed_string = ManagedString::new(content.to_owned());
            self.allocated_bytes += managed_string.size();
            let key = self.strings.insert(managed_string);

            // this is a second copy of the content :-(
            self.interned_strings.insert(content.to_owned(), key);

            AllocationResult {
                should_garbage_collect: self.should_collect(),
                index: key,
            }
        }
    }

    fn deallocate_string(&mut self, string_key: StringIndex) -> Option<ManagedString> {
        if let Some(managed_string) = self.strings.get(string_key) {
            self.interned_strings.remove(&managed_string.content);
            self.strings.remove(string_key)
        } else {
            None
        }
    }

    pub fn allocate_object(&mut self) -> AllocationResult<ObjectIndex> {
        let obj = ManagedObject::new();
        self.allocated_bytes += obj.size();
        let index = self.objects.insert(obj);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_object_with_properties(
        &mut self,
        properties: HashMap<StringIndex, Value>,
    ) -> AllocationResult<ObjectIndex> {
        let obj = ManagedObject::with_properties(properties);
        self.allocated_bytes += obj.size();
        let index = self.objects.insert(obj);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_array(&mut self, elements: Vec<Value>) -> AllocationResult<ArrayIndex> {
        let arr = ManagedArray::new(elements);
        self.allocated_bytes += arr.size();
        let index = self.arrays.insert(arr);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_function(
        &mut self,
        name: Option<StringIndex>,
        arity: u8,
        upvalue_count: u8,
        chunk: OwnedChunk,
    ) -> AllocationResult<FunctionIndex> {
        let func = ManagedFunction::new(name, arity, upvalue_count, chunk);
        self.allocated_bytes += func.size();
        let index = self.functions.insert(func);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_closure(
        &mut self,
        function: FunctionIndex,
        upvalues: Vec<UpvalueIndex>,
    ) -> AllocationResult<ClosureIndex> {
        let closure = ManagedClosure::new(function, upvalues);
        self.allocated_bytes += closure.size();
        let index = self.closures.insert(closure);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_upvalue(&mut self, stack_location: usize) -> AllocationResult<UpvalueIndex> {
        let upvalue = ManagedUpvalue::new_open(stack_location);
        self.allocated_bytes += upvalue.size();
        let index = self.upvalues.insert(upvalue);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn allocate_import(&mut self, resolved_path: &str) -> AllocationResult<ImportIndex> {
        // Check if we already have an import for this resolved path
        if let Some(&index) = self.import_lookup.get(resolved_path) {
            return AllocationResult {
                should_garbage_collect: false,
                index,
            };
        }

        // Create new import
        let path_index = self.allocate_string(resolved_path).index;
        let import = ManagedImport::new(path_index);
        self.allocated_bytes += import.size();
        let index = self.imports.insert(import);

        // Update lookup map
        self.import_lookup.insert(resolved_path.to_string(), index);

        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn load_import(&self, index: ImportIndex) -> &ManagedImport {
        self.imports.get(index).expect("Import must exist")
    }

    pub fn load_import_mut(&mut self, index: ImportIndex) -> &mut ManagedImport {
        self.imports.get_mut(index).expect("Import must exist")
    }

    pub fn allocate_binary(&mut self, data: Vec<u8>) -> AllocationResult<BinaryIndex> {
        let binary = ManagedBinary::new(data);
        self.allocated_bytes += binary.size();
        let index = self.binaries.insert(binary);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
    }

    pub fn load_binary(&self, index: BinaryIndex) -> &ManagedBinary {
        self.binaries.get(index).expect("Binary must exist")
    }

    pub fn load_binary_mut(&mut self, index: BinaryIndex) -> &mut ManagedBinary {
        self.binaries.get_mut(index).expect("Binary must exist")
    }

    /// Check if garbage collection should be triggered
    pub fn should_collect(&self) -> bool {
        #[cfg(feature = "stress_gc")]
        {
            eprintln!(
                "[MemoryManager] Stress GC enabled - triggering collection ({} bytes)",
                self.allocated_bytes
            );
            return true;
        }

        #[cfg(not(feature = "stress_gc"))]
        {
            let should_collect = self.allocated_bytes >= self.gc_threshold;
            if should_collect {
                eprintln!(
                    "[MemoryManager] Threshold exceeded - triggering collection ({} bytes >= {} bytes)",
                    self.allocated_bytes, self.gc_threshold
                );
            }
            should_collect
        }
    }

    /// Helper method to mark an upvalue and its closed value if present
    fn mark_upvalue(&mut self, upvalue_index: UpvalueIndex, values: &mut VecDeque<Value>) {
        if let Some(managed_upvalue) = self.upvalues.get_mut(upvalue_index) {
            managed_upvalue.marked.set(true);
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Marking Upvalue {:?}", upvalue_index)
            }

            // If the upvalue is closed, mark its captured value
            if let Some(closed_value) = managed_upvalue.closed_value {
                values.push_back(closed_value);
            }
        } else {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!(
                    "[MemoryManager] WARNING: Failed to mark Upvalue {:?} - not found",
                    upvalue_index
                )
            }
        }
    }

    /// Runs a mark and sweep pass starting from roots
    /// open_upvalue_roots: upvalues that are still on the open upvalues list and must be kept alive
    pub fn run_garbage_collect(
        &mut self,
        roots: Vec<Value>,
        open_upvalue_roots: Vec<UpvalueIndex>,
    ) {
        let mut values = VecDeque::from(roots);

        // Add external roots if any
        for extra_roots in &self.external_roots {
            for &value in extra_roots {
                values.push_back(value);
            }
        }

        // Mark open upvalues as roots - these haven't been captured into closures yet
        for upvalue_index in open_upvalue_roots {
            self.mark_upvalue(upvalue_index, &mut values);
        }

        // Add external upvalue roots if any
        let extra_upvalue_roots = self.external_upvalue_roots.clone();
        for extra_upvalues in extra_upvalue_roots {
            for upvalue_index in extra_upvalues {
                self.mark_upvalue(upvalue_index, &mut values);
            }
        }

        // Mark Phase, iterate over the roots, mark Values that need to remain
        while let Some(head) = values.pop_front() {
            match head {
                Value::String(string_index) => {
                    if let Some(ms) = self.strings.get_mut(string_index) {
                        if !ms.marked.get() {
                            ms.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking String {:?}", string_index)
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark String {:?} - not found",
                                string_index
                            )
                        }
                    }
                }
                Value::Object(object_index) => {
                    if let Some(managed_object) = self.objects.get_mut(object_index) {
                        if !managed_object.marked.get() {
                            managed_object.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking Object {:?}", object_index)
                            }

                            for (field_key, field_value) in &managed_object.properties {
                                values.push_back(Value::String(*field_key));
                                values.push_back(*field_value);
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Object {:?} - not found",
                                object_index
                            )
                        }
                    }
                }
                Value::Array(array_index) => {
                    if let Some(managed_array) = self.arrays.get_mut(array_index) {
                        if !managed_array.marked.get() {
                            managed_array.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking Array {:?}", array_index)
                            }

                            // Mark all elements in the array
                            for element in &managed_array.elements {
                                values.push_back(*element);
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Array {:?} - not found",
                                array_index
                            )
                        }
                    }
                }
                Value::Function(function_index) => {
                    if let Some(managed_function) = self.functions.get_mut(function_index) {
                        if !managed_function.marked.get() {
                            managed_function.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking Function {:?}", function_index)
                            }

                            // Mark the function name if present
                            if let Some(name_index) = managed_function.name {
                                values.push_back(Value::String(name_index));
                            }

                            // Mark all constants in the function's chunk
                            for constant in &managed_function.chunk.constants {
                                values.push_back(*constant);
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Function {:?} - not found",
                                function_index
                            )
                        }
                    }
                }
                Value::Closure(closure_index) => {
                    if let Some(managed_closure) = self.closures.get_mut(closure_index) {
                        if !managed_closure.marked.get() {
                            managed_closure.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking Closure {:?}", closure_index)
                            }

                            // Mark the function this closure wraps
                            values.push_back(Value::Function(managed_closure.function));

                            // Collect upvalue indices to avoid borrow checker issues
                            let upvalue_indices: Vec<UpvalueIndex> =
                                managed_closure.upvalues.clone();

                            // Mark all upvalues in this closure
                            for upvalue_index in upvalue_indices {
                                self.mark_upvalue(upvalue_index, &mut values);
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Closure {:?} - not found",
                                closure_index
                            )
                        }
                    }
                }
                Value::Import(import_index) => {
                    if let Some(managed_import) = self.imports.get_mut(import_index) {
                        if !managed_import.marked.get() {
                            managed_import.marked.set(true);
                            #[cfg(feature = "gc_debug")]
                            {
                                eprintln!("[MemoryManager] Marking Import {:?}", import_index)
                            }

                            // Mark the path string
                            values.push_back(Value::String(managed_import.path));

                            // Mark the cached result if it exists
                            if let Some(cached_value) = managed_import.cached_result {
                                values.push_back(cached_value);
                            }
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Import {:?} - not found",
                                import_index
                            )
                        }
                    }
                }
                Value::Binary(binary_index) => {
                    if let Some(managed_binary) = self.binaries.get_mut(binary_index) {
                        managed_binary.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Binary {:?}", binary_index)
                        }
                    } else {
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!(
                                "[MemoryManager] WARNING: Failed to mark Binary {:?} - not found",
                                binary_index
                            )
                        }
                    }
                }

                _ => continue,
            };
        }

        // Sweep Phase, iterate over all the different slotmaps and delete the values that are not marked.
        // For those that are marked, set them to false now.
        let mut strings_to_delete: Vec<StringIndex> = Vec::new();
        for (string_idx, managed_string) in self.strings.iter_mut() {
            if managed_string.marked.get() {
                managed_string.marked.set(false);
            } else {
                strings_to_delete.push(string_idx);
                self.allocated_bytes -= managed_string.size();
            }
        }

        let mut objects_to_delete: Vec<ObjectIndex> = Vec::new();
        for (obj_idx, obj) in self.objects.iter_mut() {
            if obj.marked.get() {
                obj.marked.set(false);
            } else {
                objects_to_delete.push(obj_idx);
                self.allocated_bytes -= obj.size();
            }
        }

        let mut arrays_to_delete: Vec<ArrayIndex> = Vec::new();
        for (arr_idx, arr) in self.arrays.iter_mut() {
            if arr.marked.get() {
                arr.marked.set(false);
            } else {
                arrays_to_delete.push(arr_idx);
                self.allocated_bytes -= arr.size();
            }
        }

        let mut functions_to_delete: Vec<FunctionIndex> = Vec::new();
        for (func_idx, func) in self.functions.iter_mut() {
            if func.marked.get() {
                func.marked.set(false);
            } else {
                functions_to_delete.push(func_idx);
                self.allocated_bytes -= func.size();
            }
        }

        let mut closures_to_delete: Vec<ClosureIndex> = Vec::new();
        for (closure_idx, closure) in self.closures.iter_mut() {
            if closure.marked.get() {
                closure.marked.set(false);
            } else {
                closures_to_delete.push(closure_idx);
                self.allocated_bytes -= closure.size();
            }
        }

        let mut upvalues_to_delete: Vec<UpvalueIndex> = Vec::new();
        for (upvalue_idx, upvalue) in self.upvalues.iter_mut() {
            if upvalue.marked.get() {
                upvalue.marked.set(false);
            } else {
                upvalues_to_delete.push(upvalue_idx);
                self.allocated_bytes -= upvalue.size();
            }
        }

        let mut imports_to_delete: Vec<ImportIndex> = Vec::new();
        for (import_idx, import) in self.imports.iter_mut() {
            if import.marked.get() {
                import.marked.set(false);
            } else {
                imports_to_delete.push(import_idx);
                self.allocated_bytes -= import.size();
            }
        }

        let mut binaries_to_delete: Vec<BinaryIndex> = Vec::new();
        for (binary_idx, binary) in self.binaries.iter_mut() {
            if binary.marked.get() {
                binary.marked.set(false);
            } else {
                binaries_to_delete.push(binary_idx);
                self.allocated_bytes -= binary.size();
            }
        }

        for string_idx in strings_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing String {:?}", string_idx)
            }
            self.deallocate_string(string_idx);
        }

        for obj_idx in objects_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Object {:?}", obj_idx)
            }
            self.objects.remove(obj_idx);
        }

        for arr_idx in arrays_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Array {:?}", arr_idx)
            }
            self.arrays.remove(arr_idx);
        }

        for func_idx in functions_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Function {:?}", func_idx)
            }
            self.functions.remove(func_idx);
        }

        for closure_idx in closures_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Closure {:?}", closure_idx)
            }
            self.closures.remove(closure_idx);
        }

        for upvalue_idx in upvalues_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Upvalue {:?}", upvalue_idx)
            }
            self.upvalues.remove(upvalue_idx);
        }

        for import_idx in imports_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Import {:?}", import_idx)
            }
            if let Some(import) = self.imports.get(import_idx) {
                let path = self.load_string(import.path).to_string();
                self.import_lookup.remove(&path);
            }
            self.imports.remove(import_idx);
        }

        for binary_idx in binaries_to_delete {
            #[cfg(feature = "gc_debug")]
            {
                eprintln!("[MemoryManager] Removing Binary {:?}", binary_idx)
            }
            self.binaries.remove(binary_idx);
        }

        self.gc_threshold = self.allocated_bytes * 2;
    }

    pub fn load_object(&self, key: ObjectIndex) -> &ManagedObject {
        self.objects
            .get(key)
            .expect(format!("Object not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_string(&self, key: StringIndex) -> &str {
        &self
            .strings
            .get(key)
            .expect(format!("String not found in SlotMap: {:?}", key).as_str())
            .content
    }

    pub fn load_array(&self, key: ArrayIndex) -> &ManagedArray {
        self.arrays
            .get(key)
            .expect(format!("Array not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_function(&self, key: FunctionIndex) -> &ManagedFunction {
        self.functions
            .get(key)
            .expect(format!("Function not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_closure(&self, key: ClosureIndex) -> &ManagedClosure {
        self.closures
            .get(key)
            .expect(format!("Closure not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_upvalue(&self, key: UpvalueIndex) -> &ManagedUpvalue {
        self.upvalues
            .get(key)
            .expect(format!("Upvalue not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_upvalue_mut(&mut self, key: UpvalueIndex) -> &mut ManagedUpvalue {
        self.upvalues
            .get_mut(key)
            .expect(format!("Upvalue not found in SlotMap: {:?}", key).as_str())
    }

    /// Get current statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (self.allocated_bytes, self.gc_threshold, self.strings.len())
    }
}

impl Default for MemoryManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_interning() {
        let mut set = MemoryManager::new();

        let s1 = set.allocate_string("hello");
        let s2 = set.allocate_string("hello");
        let s3 = set.allocate_string("world");

        // Same content should give same handle
        assert_eq!(s1, s2);
        assert_ne!(s1, s3);

        assert_eq!(set.load_string(s1.index), "hello");
    }

    #[test]
    fn test_interning_with_owned_strings() {
        let mut set = MemoryManager::new();
        let s1: AllocationResult<StringIndex>;
        let s2: AllocationResult<StringIndex>;

        {
            let owned1 = "hello".to_string();
            let owned2 = "hello".to_string();

            s1 = set.allocate_string(&owned1);
            s2 = set.allocate_string(&owned2);
        }

        // Should reuse the same interned string
        assert_eq!(s1, s2);
    }

    #[test]
    fn deallocate_string() {
        let mut manager = MemoryManager::new();
        let s = manager.allocate_string("hello");
        let repeated = manager.allocate_string("hello");

        assert_eq!(
            manager.load_string(s.index),
            manager.load_string(repeated.index)
        );

        assert_eq!(
            Some("hello"),
            manager
                .deallocate_string(repeated.index)
                .map(|s| s.content.clone())
                .as_deref()
        );

        assert_eq!(manager.strings.get(s.index), None);
        assert_eq!(manager.strings.get(repeated.index), None);
    }

    #[test]
    fn allocate_object() {
        let mut manager = MemoryManager::new();
        let name = manager.allocate_string("field1").index;
        let field_value = Value::Boolean(true);

        let mut properties = HashMap::new();

        properties.insert(name, field_value);
        let object_index = manager.allocate_object_with_properties(properties).index;

        let object = manager.load_object(object_index);
        assert_eq!(Some(&Value::Boolean(true)), object.get(&name));
    }

    #[test]
    fn garbage_collection() {
        let mut manager = MemoryManager::new();
        let name = manager.allocate_string("field1").index;
        let field_value = Value::Boolean(true);

        let mut properties = HashMap::new();

        properties.insert(name, field_value);
        let object_index = manager.allocate_object_with_properties(properties).index;

        let object_size = manager.load_object(object_index).size();
        let string_size = manager
            .strings
            .get(name)
            .expect("String field1 was just created")
            .size();

        let mut roots: Vec<Value> = Vec::new();
        roots.push(Value::Object(object_index));

        manager.run_garbage_collect(roots, vec![]);
        assert_eq!(
            object_size + string_size,
            manager.allocated_bytes,
            "Total memory should be both string and object size"
        );

        let mut only_string: Vec<Value> = Vec::new();
        only_string.push(Value::String(name));
        manager.run_garbage_collect(only_string, vec![]);
        assert_eq!(
            string_size, manager.allocated_bytes,
            "GC should have collected the object but left the string around"
        );

        // This would panic since the object was garbage collected:
        assert_eq!(None, manager.objects.get(object_index));

        manager.run_garbage_collect(vec![], vec![]);
        assert_eq!(0, manager.allocated_bytes);
        // This would panic since the string was garbage collected:
        assert_eq!(None, manager.strings.get(name));
    }
}
