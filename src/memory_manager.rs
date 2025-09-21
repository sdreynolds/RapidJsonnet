use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::cell::Cell;
use slotmap::SlotMap;
use chunk::{ObjectIndex, StringIndex, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct AllocationResult<T> {
    pub should_garbage_collect: bool,
    pub index: T
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
        let map_capacity_bytes = self.properties.capacity() * (
            std::mem::size_of::<StringIndex>() + std::mem::size_of::<Value>()
        );
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

/// A HashSet-based string interning system with garbage collection support
pub struct MemoryManager {
    /// HashSet containing all interned strings
    interned_strings: HashMap<String, StringIndex>,
    strings: SlotMap<StringIndex, ManagedString>,
    /// Collection of Objects
    objects: SlotMap<ObjectIndex, ManagedObject>,
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
                index: key
            }
        }
    }

    fn deallocate_string(&mut self, string_key: StringIndex) -> Option<ManagedString> {
        let content = self.load_string(string_key).map(|s| s.content.clone());
        if let Some(content) = content {
            self.interned_strings.remove(&content);
        }

        self.strings.remove(string_key)
    }

    pub fn allocate_object(&mut self) -> AllocationResult<ObjectIndex> {
        let obj = ManagedObject::new();
        self.allocated_bytes += obj.size();
        let index = self.objects.insert(obj);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index
        }
    }

    pub fn allocate_object_with_properties(&mut self, properties: HashMap<StringIndex, Value>) -> AllocationResult<ObjectIndex> {
        let obj = ManagedObject::with_properties(properties);
        self.allocated_bytes += obj.size();
        let index = self.objects.insert(obj);
        AllocationResult {
            should_garbage_collect: self.should_collect(),
            index
        }
    }

    /// Check if garbage collection should be triggered
    pub fn should_collect(&self) -> bool {
        #[cfg(feature = "stress_gc")]
        {
            eprintln!("[MemoryManager] Stress GC enabled - triggering collection ({} bytes)",
                     self.allocated_bytes);
            return true;
        }

        #[cfg(not(feature = "stress_gc"))]
        {
            let should_collect = self.allocated_bytes >= self.gc_threshold;
            if should_collect {
                eprintln!("[MemoryManager] Threshold exceeded - triggering collection ({} bytes >= {} bytes)",
                         self.allocated_bytes, self.gc_threshold);
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
                    self.strings.get_mut(string_index).map(|ms| ms.marked.set(true));
                    #[cfg(feature = "gc_debug")]
                    {
                        eprintln!("[MemoryManager] Marking String {}", self.load_string(string_index).content.as_str())
                    }
                },
                Value::Object(object_index) => {
                    self.objects.get_mut(object_index).map(|managed_object| {
                        managed_object.marked.set(true);
                        #[cfg(feature = "gc_debug")]
                        {
                            eprintln!("[MemoryManager] Marking Object {:?}", object_index)
                        }

                        for field_index in managed_object.properties.keys() {
                            values.push_back(Value::String(*field_index));
                        }
                    });
                },

                _ => continue
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

        for string_idx in strings_to_delete {
            self.strings.remove(string_idx);
        }

        for obj_idx in objects_to_delete {
            self.objects.remove(obj_idx);
        }

        self.gc_threshold = self.allocated_bytes * 2;
    }

    pub fn load_object(&self, key: ObjectIndex) -> Option<&ManagedObject> {
        self.objects.get(key)
    }

    pub fn load_string(&self, key: StringIndex) -> Option<&ManagedString> {
        self.strings.get(key)
    }

    /// Get current statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.allocated_bytes,
            self.gc_threshold,
            self.strings.len()
        )
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

        assert_eq!(set.load_string(s1.index).map(|managed| managed.content.as_str()), Some("hello"));
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

        assert_eq!(manager.load_string(s.index), manager.load_string(repeated.index));

        assert_eq!(Some("hello"),
                   manager
                   .deallocate_string(repeated.index)
                   .map(|s| s.content.clone()).as_deref());

        assert_eq!(manager.load_string(s.index), None);
        assert_eq!(manager.load_string(repeated.index), None);

    }

    #[test]
    fn allocate_object() {
        let mut manager = MemoryManager::new();
        let name = manager.allocate_string("field1").index;
        let field_value = Value::Boolean(true);

        let mut properties = HashMap::new();

        properties.insert(name, field_value);
        let object_index = manager.allocate_object_with_properties(properties).index;

        if let Some(object) = manager.load_object(object_index) {
            assert_eq!(Some(&Value::Boolean(true)), object.get(&name));
        } else {
            panic!("Failed to load the created object");
        }
    }

    #[test]
    fn garbage_collection() {
        let mut manager = MemoryManager::new();
        let name = manager.allocate_string("field1").index;
        let field_value = Value::Boolean(true);

        let mut properties = HashMap::new();

        properties.insert(name, field_value);
        let object_index = manager.allocate_object_with_properties(properties).index;

        let object_size = manager.load_object(object_index)
            .expect("Object was just created").size();
        let string_size = manager.load_string(name)
            .expect("String field1 was just created").size();

        let mut roots: Vec<Value> = Vec::new();
        roots.push(Value::Object(object_index));

        manager.run_garbage_collect(roots);
        assert_eq!(object_size + string_size, manager.allocated_bytes,
                   "Total memory should be both string and object size");

        let mut only_string: Vec<Value> = Vec::new();
        only_string.push(Value::String(name));
        manager.run_garbage_collect(only_string);
        assert_eq!(string_size, manager.allocated_bytes,
                   "GC should have collected the object but left the string around");

        assert_eq!(None, manager.load_object(object_index),
                   "GC should have removed the object from the slotmap");

        manager.run_garbage_collect(vec!());
        assert_eq!(0, manager.allocated_bytes);
        assert_eq!(None, manager.load_string(name));


    }
}
