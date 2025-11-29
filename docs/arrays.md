# Array Support
In this task, it is time to add array support to the implementation. The support will touch most of the code in different ways and will require some planning. The parser will need to parse the `[` and `]` and will need to recursively parse the rest of the values.

## Garbage Collection
To make this all function, we need a `ManagedArray` in the `memory_manager.rs`. The `ManagedArray` should be managed by `MemoryManager` as a `arrays` property. This should be a `SlotMap` that uses an `ArrayIndex`. `ArrayIndex` should be a `pub type ArrayIndex = DefaultKey`. A `ManagedArray` is a `vector` of `Value` objects and that `vector` should be passed into the `allocate_array` method of the `MemoryManager`. `allocate_array` method *must* return a `AllocationResult` object back to the caller.

### Tracing
The garbage collection is a tracing algorithm and the mark phase of the trace needs to iterate over the `Vector<Value>` for the `ManagedArray` and mark each one as `marked` using the same system as `ManagedString` and `ManagedObject`

## VirtualMachine
The VirtualMachine will process the `CreateArray` Opcode for array creation an process the `element_count` in the operand. It will then create a vector of `element_count` size, and then pop all the items off the stack. An empty array -- an `CreateArray` Opcode with a `0` as an operand -- is valid and expected. In this instance, the `ManagedArray` has a vector of `0`. After creating a `ManagedArray`, the `Value::Array` is pushed onto the stack and *then* the `AllocationResult` is checked for `should_garbage_collect` boolean.

### Pre-allocate and Fill Backwards

// In VirtualMachine::execute for CreateArray
let mut elements = Vec::with_capacity(element_count);
elements.resize(element_count, Value::Null); // or use unsafe with MaybeUninit
 for i in (0..element_count).rev() {
    elements[i] = self.pop();
}

 Why this over pop-then-reverse:
 | Approach       | Allocations             | Copies | Cache behavior             |
|----------------|-------------------------|--------|----------------------------|
| Pop + reverse  | 1 vec + reverse shuffle | 2N     | Poor (reverse touches all) |
| Fill backwards | 1 vec                   | N      | Good (sequential write)    |
 The backwards-fill approach:
- Single allocation with known capacity
- Each element written exactly once
- No need for Vec::reverse() which is O(n) additional work
- Matches how Go-jsonnet and other interpreters handle this

## Compiler
The compiler will be responsible for emitting the opcode `CreateArray` when facing an `[` Token. It will push all the values onto the stack and set that value up. It *must* recursively handle parsing within the array. Trailing commas are allowed, missing commas are a compiler error and missing values is a compiler error.

## Examples

``` jsonnet
[
  1, 2 ,3,
  5,
]
```

``` jsonnet
[
  "sup", {a: true, b: false, c: "yep", "detroit": 15 + 16 / (8 * 9)}
]
```

## Array Concatentation
Arrays can be concatenated together to form new arrays. The opcode exists and the `VirtualMachine` should pop the two arrays and concat them together.

## ArrayIndex
- Opcode: ArrayIndex (already exists)
- Stack: [..., array, index] → pops both, pushes array[index]
- Compile-time error: literal negative index like arr[-1]
- Runtime errors: index out of bounds, negative, or non-integer
- Precedence: highest level (1), same as function calls and field access


## Manifestation
To manifest an `Value::Array(ArrayIndex)`, use `serde_json` and have call `value_to_json` recursively on the elements.
