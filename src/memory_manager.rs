use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::cell::Cell;
use slotmap::{SlotMap, DefaultKey};
use chunk::Value;

/// A Jsonnet object containing property key-value pairs
#[derive(Debug, Clone, PartialEq)]
pub struct ManagedObject {
    /// Object properties mapping interned string keys to values
    /// TODO Properties is wrong here. It Should be a pointer into the SlotMap for string
    pub properties: HashMap<ManagedString, Value>,
    /// Allocated bytes for GC accounting (includes HashMap capacity overhead)
    allocated_bytes: usize,
    // GC marking
    marked: Cell<bool>,
}

impl ManagedObject {
    /// Calculate the actual size of this object including HashMap overhead
    fn calculate_size(&self) -> usize {
        let base_size = std::mem::size_of::<Self>();
        // HashMap capacity accounts for actual allocated memory, not just length
        let map_capacity_bytes = self.properties.capacity() * (
            std::mem::size_of::<ManagedString>() + std::mem::size_of::<Value>()
        );
        base_size + map_capacity_bytes
    }

    /// Create a new empty Jsonnet object
    pub fn new() -> Self {
        let properties = HashMap::new();
        let mut obj = Self {
            properties,
            allocated_bytes: 0,
            marked: Cell::new(false),
        };
        obj.allocated_bytes = obj.calculate_size();
        obj
    }

    /// Create a Jsonnet object with the given properties
    pub fn with_properties(properties: HashMap<ManagedString, Value>) -> Self {
        let mut obj = Self {
            properties,
            allocated_bytes: 0,
            marked: Cell::new(false),
        };
        obj.allocated_bytes = obj.calculate_size();
        obj
    }

    /// Get a property value by key
    pub fn get(&self, key: &ManagedString) -> Option<&Value> {
        self.properties.get(key)
    }

    /// Get the allocated byte size of this object
    pub fn allocated_bytes(&self) -> usize {
        self.allocated_bytes
    }

    /// Check if object has a property
    pub fn has_property(&self, key: &ManagedString) -> bool {
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
    interned_strings: HashMap<String, DefaultKey>,
    strings: SlotMap<DefaultKey, ManagedString>,
    /// Collection of Objects
    objects: SlotMap<DefaultKey, ManagedObject>,
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
    pub fn allocate_string(&mut self, content: &str) -> DefaultKey {
        // Check if already interned by creating a temporary handle for lookup
        if let Some(index) = self.interned_strings.get(content) {
            *index
        } else {
            let managed_string = ManagedString::new(content.to_owned());
            self.allocated_bytes += managed_string.size();
            let key = self.strings.insert(managed_string);

            // this is a second copy of the content :-(
            self.interned_strings.insert(content.to_owned(), key);

            key
        }
    }

    fn deallocate_string(&mut self, string_key: DefaultKey) -> Option<ManagedString> {
        let content = self.load_string(string_key).map(|s| s.content.clone());
        if let Some(content) = content {
            self.interned_strings.remove(&content);
        }

        self.strings.remove(string_key)
    }

    pub fn allocate_object(&mut self) -> DefaultKey {
        let obj = ManagedObject::new();
        self.allocated_bytes += obj.calculate_size();
        self.objects.insert(obj)
    }

    pub fn allocate_object_with_properties(&mut self, properties: HashMap<ManagedString, Value>) -> DefaultKey {
        let obj = ManagedObject::with_properties(properties);
        self.allocated_bytes += obj.calculate_size();
        self.objects.insert(obj)
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

    pub fn load_object(&self, key: DefaultKey) -> Option<&ManagedObject> {
        self.objects.get(key)
    }

    pub fn load_string(&self, key: DefaultKey) -> Option<&ManagedString> {
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

        assert_eq!(set.load_string(s1).map(|managed| managed.content.as_str()), Some("hello"));
    }

    #[test]
    fn test_interning_with_owned_strings() {
        let mut set = MemoryManager::new();
        let s1: DefaultKey;
        let s2: DefaultKey;

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

        assert_eq!(manager.load_string(s), manager.load_string(repeated));

        assert_eq!(Some("hello"),
                   manager
                   .deallocate_string(repeated)
                   .map(|s| s.content.clone()).as_deref());

        assert_eq!(manager.load_string(s), None);
        assert_eq!(manager.load_string(repeated), None);

    }

}
