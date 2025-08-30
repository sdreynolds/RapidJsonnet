use std::collections::HashMap;
use std::cell::Cell;

/// An interned string that points to shared string data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InternedString {
    ptr: *const InternedStringData,
}

/// Internal string data with GC marking support
struct InternedStringData {
    content: Box<str>,
    marked: Cell<bool>,  // GC mark bit
    size: usize,         // String byte size for accounting
}

impl InternedStringData {
    fn new(content: &str) -> Self {
        Self {
            content: content.into(),
            marked: Cell::new(false),
            size: content.len(),
        }
    }

    fn new_owned(content: String) -> Self {
        let size = content.len();
        Self {
            content: content.into_boxed_str(),
            marked: Cell::new(false),
            size,
        }
    }
}

/// Global string pool with Mark & Sweep garbage collection
pub struct StringPool {
    strings: HashMap<String, InternedString>,
    all_strings: Vec<InternedString>,
    bytes_allocated: usize,
    next_garbage_collection: usize,
    gray_list: Vec<InternedString>,
}

impl StringPool {
    pub fn new() -> Self {
        Self {
            strings: HashMap::new(),
            all_strings: Vec::new(),
            bytes_allocated: 0,
            next_garbage_collection: 1024 * 1024, // Initial 1MB threshold
            gray_list: Vec::new(),
        }
    }

    /// Intern a string, returning an InternedString reference
    pub fn intern(&mut self, content: &str) -> InternedString {
        // Check if already interned
        if let Some(&interned) = self.strings.get(content) {
            return interned;
        }

        // Create new interned string
        let size = content.len();
        let data = Box::new(InternedStringData::new(content));
        let interned = InternedString {
            ptr: Box::leak(data) as *const InternedStringData
        };

        // Track in collections
        self.strings.insert(content.to_string(), interned);
        self.all_strings.push(interned);
        self.bytes_allocated += size;

        interned
    }

    /// GC-aware string interning that triggers collection if needed
    pub fn intern_with_gc(&mut self, content: &str, additional_roots: Vec<InternedString>) -> InternedString {
        // During compilation, be conservative about GC to avoid collecting needed strings
        // Only collect if we have proper root information
        if self.should_collect() && !additional_roots.is_empty() {
            self.collect_garbage(additional_roots);
        }

        // Perform the allocation (allocation tracking is already done in intern)
        self.intern(content)
    }

    /// Intern a string by moving ownership, returning an InternedString reference
    fn intern_owned(&mut self, content: String) -> InternedString {
        // Check if already interned
        if let Some(&interned) = self.strings.get(&content) {
            return interned;
        }

        // Create new interned string using move semantics
        let size = content.len();
        let data = Box::new(InternedStringData::new_owned(content.clone()));
        let interned = InternedString {
            ptr: Box::leak(data) as *const InternedStringData
        };

        // Track in collections (move happens here)
        self.strings.insert(content, interned);
        self.all_strings.push(interned);
        self.bytes_allocated += size;

        interned
    }

    /// GC-aware string interning by moving ownership
    pub fn intern_owned_with_gc(&mut self, content: String, additional_roots: Vec<InternedString>) -> InternedString {
        // During compilation, be conservative about GC to avoid collecting needed strings
        // Only collect if we have proper root information
        if self.should_collect() && !additional_roots.is_empty() {
            self.collect_garbage(additional_roots);
        }

        // Perform the allocation (allocation tracking is already done in intern_owned)
        self.intern_owned(content)
    }

    /// Remove a string from the pool (used during GC sweep)
    pub fn deallocate_string(&mut self, string: InternedString) {
        let data = unsafe { &*string.ptr };
        self.bytes_allocated -= data.size;

        // Remove from collections
        let content_key = data.content.to_string();
        self.strings.remove(&content_key);
        self.all_strings.retain(|&s| s.ptr != string.ptr);

        // Actually deallocate the box
        unsafe {
            let _boxed = Box::from_raw(string.ptr as *mut InternedStringData);
            // Box automatically drops when it goes out of scope
        };
    }

    /// Check if garbage collection should be triggered
    pub fn should_collect(&self) -> bool {
        #[cfg(feature = "stress_gc")]
        {
            eprintln!("[GC] Stress GC enabled - triggering collection (allocated: {} bytes, {} strings)",
                     self.bytes_allocated, self.all_strings.len());
            return true;
        }

        let should_collect = self.bytes_allocated >= self.next_garbage_collection;
        if should_collect {
            eprintln!("[GC] Threshold exceeded - triggering collection (allocated: {} bytes >= threshold: {} bytes, {} strings)",
                     self.bytes_allocated, self.next_garbage_collection, self.all_strings.len());
        }
        should_collect
    }

    /// Perform Mark & Sweep garbage collection
    pub fn collect_garbage(&mut self, roots: Vec<InternedString>) {
        eprintln!("[GC] Starting collection with {} roots", roots.len());

        let initial_count = self.all_strings.len();
        let initial_bytes = self.bytes_allocated;

        eprintln!("[GC] Mark phase: Processing {} roots from {} total strings ({} bytes)",
                 roots.len(), initial_count, initial_bytes);

        // Mark phase: Start with roots (gray list)
        self.gray_list.clear();
        for root in roots {
            self.mark_gray(root);
        }

        let marked_count = self.gray_list.len();
        eprintln!("[GC] Mark phase: {} strings marked as reachable", marked_count);

        // Process gray list until empty (mark black)
        while let Some(string) = self.gray_list.pop() {
            self.mark_black(string);
        }

        // Sweep phase: Deallocate unmarked strings
        let mut to_deallocate = Vec::new();
        for &string in &self.all_strings {
            let data = unsafe { &*string.ptr };
            if !data.marked.get() {
                to_deallocate.push(string);
            } else {
                // Reset mark for next collection
                data.marked.set(false);
            }
        }

        let deallocate_count = to_deallocate.len();
        eprintln!("[GC] Sweep phase: Deallocating {} unmarked strings", deallocate_count);

        for string in to_deallocate {
            self.deallocate_string(string);
        }

        let final_count = self.all_strings.len();
        let final_bytes = self.bytes_allocated;
        let old_threshold = self.next_garbage_collection;

        // Update threshold: current size * 2
        self.next_garbage_collection = std::cmp::max(
            self.bytes_allocated * 2,
            1024 * 1024 // Minimum 1MB threshold
        );

        eprintln!("[GC] Complete: {} -> {} strings, {} -> {} bytes, threshold: {} -> {} bytes",
                 initial_count, final_count, initial_bytes, final_bytes,
                 old_threshold, self.next_garbage_collection);
    }

    /// Add string to gray list if not already marked
    fn mark_gray(&mut self, string: InternedString) {
        let data = unsafe { &*string.ptr };
        if !data.marked.get() {
            self.gray_list.push(string);
        }
    }

    /// Mark string as black (reachable)
    fn mark_black(&self, string: InternedString) {
        let data = unsafe { &*string.ptr };
        data.marked.set(true);

        // Future: Mark referenced objects when we have composite types
        // For strings, there are no references to other objects
    }

    /// Get string content (for debugging/display)
    pub fn get_content(&self, string: InternedString) -> &str {
        let data = unsafe { &*string.ptr };
        &data.content
    }

    /// Get allocation statistics
    pub fn stats(&self) -> (usize, usize, usize) {
        (
            self.bytes_allocated,
            self.next_garbage_collection,
            self.all_strings.len()
        )
    }
}

impl Drop for StringPool {
    fn drop(&mut self) {
        eprintln!("[StringPool] Deallocating {} remaining strings on drop", self.all_strings.len());

        // Deallocate all remaining interned strings to prevent memory leaks
        for &string in &self.all_strings {
            unsafe {
                // Convert the raw pointer back to a Box and let it drop
                let _boxed = Box::from_raw(string.ptr as *mut InternedStringData);
                // Box automatically drops and deallocates when it goes out of scope
            }
        }

        eprintln!("[StringPool] Cleanup complete - {} bytes deallocated", self.bytes_allocated);
    }
}

// Safety: InternedString is safe to send between threads as long as
// the string pool is properly synchronized
unsafe impl Send for InternedString {}
unsafe impl Sync for InternedString {}

impl InternedString {
    /// Get the string content
    pub fn as_str(self) -> &'static str {
        let data = unsafe { &*self.ptr };
        // SAFETY: The string data lives as long as it's in the intern pool,
        // and we never deallocate while references exist (thanks to GC)
        unsafe { std::mem::transmute::<&str, &'static str>(&data.content) }
    }

    /// Check if two interned strings are equal (O(1) pointer comparison)
    pub fn ptr_eq(self, other: InternedString) -> bool {
        self.ptr == other.ptr
    }
}

impl std::fmt::Display for InternedString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_interning() {
        let mut pool = StringPool::new();

        let s1 = pool.intern("hello");
        let s2 = pool.intern("hello");
        let s3 = pool.intern("world");

        // Same content should give same interned string
        assert!(s1.ptr_eq(s2));
        assert!(!s1.ptr_eq(s3));

        // Content should be accessible
        assert_eq!(pool.get_content(s1), "hello");
        assert_eq!(pool.get_content(s3), "world");
    }

    #[test]
    fn test_garbage_collection() {
        let mut pool = StringPool::new();

        let s1 = pool.intern("keep");
        let _s2 = pool.intern("discard");

        // Only keep s1 as root
        pool.collect_garbage(vec![s1]);

        // s1 should still be accessible, s2 should be gone
        assert_eq!(pool.get_content(s1), "keep");

        // Pool should only contain the kept string
        let (_, _, count) = pool.stats();
        assert_eq!(count, 1);
    }
}
