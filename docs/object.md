# Object support
In this task, it is time to add object support to the implementation. This support will touch most of the code in different ways and will require some planning before executing. To simplify the task, an object has an `InternedString` to `Value` map. This will be tricky because an Object is also a `Value` instance and therefore the Rust compiler will require a reference counting or slots. In our implementation we will use the `slotmap` library for Jsonnet objects.

# slotmap library
This library provides a container with persistent unique keys to access stored values, SlotMap. Upon insertion a key is returned that can be used to later access or remove the values. Insertion, removal and access all take O(1) time with low overhead. Great for storing collections of objects that need stable, safe references but have no clear ownership otherwise, such as game entities or graph nodes.

The difference between a BTreeMap or HashMap and a slot map is that the slot map generates and returns the key when inserting a value. A key is always unique and will only refer to the value that was inserted. A slot map’s main purpose is to simply own things in a safe and efficient manner.

You can also create (multiple) secondary maps that can map the keys returned by SlotMap to other values, to associate arbitrary data with objects stored in slot maps, without hashing required - it’s direct indexing under the hood.

## Creating objects
- When the chunk creates an object, allocate it in the constants table and add it to the slotmap
- When concatenating objects together in virtual machine, allocate the object onto the slotmap for these objects

## When creating an object
The object will be allocated using the `slotmap` library and the `Value` enum will track the `key` returned from the `insert` method of the `slotmap`. When accessing the `Object` from the stack, the `key` will be used to reach into the `slotmap`. The `slotmap` is a property of the `VirtualMachine` and is deallocated when the `VirtualMachine` is deallocated.

### Hybrid approach (Option B) - Chosen Architecture
  - Keep current string interning system
  - Only objects use SlotMap
  - Two separate GC systems

#### Rationale for Hybrid Approach

After analysis of different approaches for memory management consistency, the hybrid approach was chosen for the following reasons:

**Performance Benefits:**
- String operations are extremely frequent in Jsonnet (property access, concatenation, comparison)
- Direct `InternedString` access via pointer is faster than SlotMap indirection (`pool.get(key)`)
- Current O(1) string equality via pointer comparison is preserved
- String interning system is already optimized and battle-tested

**Memory Efficiency:**
- Strings are typically small and numerous - direct embedding is more cache-friendly
- No additional SlotMap key storage overhead for strings
- Current string pool memory layout optimized for frequent access patterns

**Implementation Complexity:**
- Existing string interning system is working well and well-tested
- Objects can be added incrementally without disrupting string performance
- Reduces risk of introducing bugs in critical string operations
- Two focused GC systems vs one complex unified system

#### Technical Implementation Details

**Value Enum Structure:**
```rust
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(InternedString),    // Direct embedding - current system
    Object(DefaultKey),        // SlotMap key - new system
}
```

**SlotMap Integration:**
- Objects use `slotmap::DefaultKey` for indirection
- SlotMap is owned by VirtualMachine instance
- Object structure: `HashMap<InternedString, Value>` for properties

**Garbage Collection Coordination:**
- String GC: Current Mark & Sweep with `InternedString` marking
- Object GC: New Mark & Sweep with SlotMap key marking
- **Coordination**: When processing objects in GC, mark both:
  - Object property keys (`InternedString`) via string GC system
  - Object property values (`Value`) recursively, including nested objects
- **Root Collection**: Extended `collect_gc_roots()` gathers both string and object roots
- **Trigger Logic**: Either string threshold OR object threshold can trigger combined GC

**Memory Management:**
- Strings: Global string pool with Mutex synchronization
- Objects: VirtualMachine-local SlotMap (no cross-VM sharing needed)
- Lifetime: Objects deallocated when VirtualMachine drops, strings persist globally

##### Implementation order
- Refactor `main.rs`, `chunk.rs`, and `virtual_machine.rs`. `main.rs` should construct a `VirtualMachine` with the `content` and `source_id`. The `VirtualMachine` constructor should create both the `Scanner` and the `Compiler`.
- This refactor then allows the `VirtualMachine` to create the `slotmap` required for `Value::Object(DefaultKey)` and to pass the `slotmap` into the `Compiler`. The `Compiler` will insert compiled objects into the `SlotMap` and place `Object(DefaultKey)` into the `Compiler`'s constants.
- Construct the `Object` Structure with just visible fields.
- Update `Compiler` to compile objects and insert them into `constants` and have the `VirtualMachine` be able to return them
- Add transformation from `Value::Object(DefaultKey)` to `serde_json::Object(Map<String, serde_json::Value>),`
- Run tests to ensure the `Compiler` is able to define constants and have them returned by the `VirtualMachine`
- Add Compiler and VirtualMachine integration with `ObjectIndex` `opcode` to access and return fields from a defined object
- Add Compiler and VirtualMachine integration with `ObjectMerge` `opcode` to combine two `Objects` together

#### Alternative Approaches Considered

**Option A: Full SlotMap Migration**
- Migrate strings to SlotMap for consistency
- Single unified GC system
- Rejected due to string performance regression

**Option C: Enhanced Hybrid**
- Wrap `InternedString` in SlotMap key
- Partial unification of GC systems
- Rejected due to added complexity with minimal benefit

The hybrid approach provides the best balance of performance, implementation safety, and incremental development capability.

## Garbage Collection of Objects

Similar to strings, garbage collection needs to happen for objects. This implementation will coordinate with the existing string GC system.

### GC Implementation Details

**Object Marking Algorithm:**
```rust
fn mark_object(&mut self, key: DefaultKey) {
    if let Some(obj) = self.objects.get(key) {
        // Mark all property keys (InternedStrings) via string GC
        for (prop_key, prop_value) in &obj.properties {
            self.mark_string_root(*prop_key);
            self.mark_value_recursive(prop_value);
        }
    }
}

fn mark_value_recursive(&mut self, value: &Value) {
    match value {
        Value::String(interned_str) => self.mark_string_root(*interned_str),
        Value::Object(obj_key) => self.mark_object(*obj_key),
        _ => {} // Primitives don't need marking
    }
}
```

**Root Collection Strategy:**
- **VM Stack Objects**: Collect SlotMap keys directly from stack values
- **Chunk Constants**: Gather object keys stored in constants table  
- **Object Properties**: Recursively traverse object property values for nested references

**GC Coordination:**
- When processing an `Object` on the gray list, add each `InternedString` in the object's property keys to the string GC gray list to keep them alive
- Add all `Value` instances in the object's properties hashmap to the gray list for recursive marking
- Coordinate GC triggers: either string threshold OR object threshold can initiate combined collection

**Object Thresholds:**
- Track bytes allocated for objects separately from strings
- Use similar threshold doubling strategy as string GC
- Consider object size in allocation accounting (object overhead + property storage)

### Error Handling Strategy

**Object Access Errors:**
- **Invalid Property Access**: Handle `obj.nonexistent_field` gracefully with null or error
- **Type Errors**: Clear error messages for non-object property access attempts
- **Null Reference**: Safe handling of null object dereference

**SlotMap Management:**
- **Key Validity**: Verify SlotMap keys are valid before access
- **Memory Allocation**: Handle SlotMap allocation failures gracefully
- **Capacity Limits**: Define maximum object count and size limits

**Circular Reference Detection:**
- **Serialization**: Detect cycles during JSON export
- **GC Safety**: Ensure GC marking handles circular object references
- **Stack Overflow Prevention**: Limit recursion depth in object operations

### Object Operations Support

**Object Creation:**
- **Object Literals**: `{field1: value1, field2: value2}` generates CreateObject opcode
- **Chunk Constants**: Objects stored in constants table with SlotMap key reference
- **Runtime Creation**: Objects allocated in VM SlotMap during execution

**Object Access:**
- **Property Access**: `obj.field` generates ObjectIndex opcode with field name lookup  
- **Field Resolution**: Support for hidden fields (`:`, `::`, `:::` visibility)
- **Dynamic Access**: Runtime property name resolution with InternedString keys

**Object Merging:**
- **Object Concatenation**: `obj1 + obj2` generates ObjectMerge opcode
- **Field Override**: Right-hand side object properties override left-hand side
- **Recursive Merging**: Nested objects merged recursively, not replaced

**Object Serialization:**
- **JSON Export**: Convert SlotMap objects to `serde_json::Value` for output
- **Circular Reference Handling**: Detect and handle object cycles during serialization
- **Property Ordering**: Maintain consistent property ordering for deterministic output
