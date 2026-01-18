use chunk::{ArrayIndex, ClosureIndex, FunctionIndex, ObjectIndex, StringIndex, Value};
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

/// A Jsonnet function with bytecode and parameter information
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedFunction {
    /// Number of positional parameters
    pub param_count: u8,
    /// Names of parameters with their defaults (None if no default)
    pub param_names: Vec<StringIndex>,
    pub param_defaults: Vec<Option<Value>>,
    /// Bytecode offset where the function body begins
    pub code_offset: usize,
    /// GC marking
    marked: Cell<bool>,
}

impl ManagedFunction {
    /// Calculate the size of this function for GC
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        let names_capacity = self.param_names.capacity() * std::mem::size_of::<StringIndex>();
        let defaults_capacity = self.param_defaults.capacity() * std::mem::size_of::<Option<Value>>();
        base_size + names_capacity + defaults_capacity
    }

    /// Create a new function
    pub fn new(
        param_count: u8,
        param_names: Vec<StringIndex>,
        param_defaults: Vec<Option<Value>>,
        code_offset: usize,
    ) -> Self {
        Self {
            param_count,
            param_names,
            param_defaults,
            code_offset,
            marked: Cell::new(false),
        }
    }
}

/// A closure captures a function with its lexical environment
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedClosure {
    /// Reference to the function
    pub function: FunctionIndex,
    /// Captured environment: variable name -> value
    pub captured_env: HashMap<StringIndex, Value>,
    /// GC marking
    marked: Cell<bool>,
}

impl ManagedClosure {
    /// Calculate the size of this closure for GC
    fn size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        let map_capacity = self.captured_env.capacity()
            * (std::mem::size_of::<StringIndex>() + std::mem::size_of::<Value>());
        base_size + map_capacity
    }

    /// Create a new closure
    pub fn new(function: FunctionIndex, captured_env: HashMap<StringIndex, Value>) -> Self {
        Self {
            function,
            captured_env,
            marked: Cell::new(false),
        }
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
    /// Total bytes allocated for all strings
    allocated_bytes: usize,
    /// GC threshold for triggering collection
    gc_threshold: usize,
}

impl MemoryManager {
    /// Create a new string interning set
    pub fn new() -> Self {
        Self {
            interned_strings: HashMap::new(),
            strings: SlotMap::new(),
            objects: SlotMap::new(),
            arrays: SlotMap::new(),
            functions: SlotMap::new(),
            closures: SlotMap::new(),
            allocated_bytes: 0,
            gc_threshold: 1024 * 1024, // 1MB initial threshold
        }
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
        param_count: u8,
        param_names: Vec<StringIndex>,
        param_defaults: Vec<Option<Value>>,
        code_offset: usize,
    ) -> AllocationResult<FunctionIndex> {
        let func = ManagedFunction::new(param_count, param_names, param_defaults, code_offset);
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
        captured_env: HashMap<StringIndex, Value>,
    ) -> AllocationResult<ClosureIndex> {
        let closure = ManagedClosure::new(function, captured_env);
        self.allocated_bytes += closure.size();
        let index = self.closures.insert(closure);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index,
        }
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

    /// Runs a mark and sweep pass starting from roots
    pub fn run_garbage_collect(&mut self, roots: Vec<Value>) {
        let mut values = VecDeque::from(roots);

        // Mark Phase, iterate over the roots, mark Values that need to remain
        while let Some(head) = values.pop_front() {
            match head {
                Value::String(string_index) => {
                    if let Some(ms) = self.strings.get_mut(string_index) {
                        ms.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking String {:?}", string_index)
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
                        managed_object.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Object {:?}", object_index)
                        }

                        for (field_key, field_value) in &managed_object.properties {
                            values.push_back(Value::String(*field_key));
                            values.push_back(*field_value);
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
                        managed_array.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Array {:?}", array_index)
                        }

                        // Mark all elements in the array
                        for element in &managed_array.elements {
                            values.push_back(*element);
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
                        managed_function.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Function {:?}", function_index)
                        }

                        // Mark parameter names and defaults
                        for param_name in &managed_function.param_names {
                            values.push_back(Value::String(*param_name));
                        }
                        for default in &managed_function.param_defaults {
                            if let Some(default_val) = default {
                                values.push_back(*default_val);
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
                        managed_closure.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Closure {:?}", closure_index)
                        }

                        // Mark the function and all captured environment values
                        values.push_back(Value::Function(managed_closure.function));
                        for (_, value) in &managed_closure.captured_env {
                            values.push_back(*value);
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

    pub fn load_function_mut(&mut self, key: FunctionIndex) -> &mut ManagedFunction {
        self.functions
            .get_mut(key)
            .expect(format!("Function not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_closure(&self, key: ClosureIndex) -> &ManagedClosure {
        self.closures
            .get(key)
            .expect(format!("Closure not found in SlotMap: {:?}", key).as_str())
    }

    pub fn load_closure_mut(&mut self, key: ClosureIndex) -> &mut ManagedClosure {
        self.closures
            .get_mut(key)
            .expect(format!("Closure not found in SlotMap: {:?}", key).as_str())
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

        manager.run_garbage_collect(roots);
        assert_eq!(
            object_size + string_size,
            manager.allocated_bytes,
            "Total memory should be both string and object size"
        );

        let mut only_string: Vec<Value> = Vec::new();
        only_string.push(Value::String(name));
        manager.run_garbage_collect(only_string);
        assert_eq!(
            string_size, manager.allocated_bytes,
            "GC should have collected the object but left the string around"
        );

        // This would panic since the object was garbage collected:
        assert_eq!(None, manager.objects.get(object_index));

        manager.run_garbage_collect(vec![]);
        assert_eq!(0, manager.allocated_bytes);
        // This would panic since the string was garbage collected:
        assert_eq!(None, manager.strings.get(name));
    }
}
