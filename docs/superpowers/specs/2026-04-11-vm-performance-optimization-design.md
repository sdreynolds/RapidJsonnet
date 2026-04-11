# VM Performance Optimization Pass

**Date:** 2026-04-11  
**Status:** Approved

## Background

Benchmark comparison against GoJsonnet (`gen_big_object.jsonnet`, n=2000) revealed RapidJsonnet running ~3.5× slower than GoJsonnet. Profiling the execution trace identified three independent bottlenecks:

1. **O(n²) GC root rebuilding** in all std iteration functions
2. **Per-element bytecode chunk allocation** in `std.makeArray`
3. **Unnecessary string cloning** in `std.join` (string mode)

This spec covers all three fixes as a single optimization pass.

---

## Bottleneck 1: O(n²) GC Root Rebuilding

### Problem

Every std iteration function (`std.map`, `std.filter`, `std.foldl`, `std.mapWithIndex`, `std.sort`, `std.setUnion`, `std.setInter`, `std.setDiff`, `std.mapWithKey`, `std.mapKeys`, and more) follows this pattern inside its loop body:

```rust
for &elem in &elements {
    let mut roots = Vec::new();
    roots.extend_from_slice(&elements);   // ALL n elements
    roots.extend_from_slice(&results);    // Growing: 0..n
    roots.push(func_val);
    roots.push(elem);
    let mut upvalue_roots = Vec::new();
    // ... walk open_upvalues linked list ...
    self.memory_manager.push_external_roots(roots, upvalue_roots);
    let result = self.call_value_with_one_arg(func_val, elem);
    self.memory_manager.pop_external_roots();
    results.push(result?);
}
```

For n elements this allocates O(n²) total Vec entries. For `std.map` over the 4000-element array in `gen_big_object.jsonnet`, this copies ~8 million `Value`s (each 8 bytes) across the full run. The same pattern appears at 20+ call sites in `virtual_machine.rs`.

### Fix

Restructure all iteration loops to push GC roots **once** before the loop using a pre-allocated, GC-managed result array:

**Step 1 — Push input roots once before the loop:**
```rust
let upvalue_roots = collect_open_upvalue_roots(&self);  // once, not per-iter
self.memory_manager.push_external_roots(
    [elements.as_slice(), &[func_val]].concat(),
    upvalue_roots,
);
```

**Step 2 — Pre-allocate a GC-managed result array:**
```rust
let result_arr_idx = self.memory_manager
    .allocate_array(vec![Value::Null; n])
    .index;
self.memory_manager.push_external_roots(
    vec![Value::Array(result_arr_idx)],
    vec![],
);
```
GC can always trace this array and all values written into it.

**Step 3 — Fill in-place during the loop:**
```rust
for i in 0..n {
    let result = self.call_value_with_one_arg(func_val, elements[i])?;
    self.memory_manager.set_array_element(result_arr_idx, i, result);
}
```

**Step 4 — Pop both root frames after the loop:**
```rust
self.memory_manager.pop_external_roots(); // result array frame
self.memory_manager.pop_external_roots(); // elements + func frame
```

**For variable-length results (e.g., `std.filter`):** Pre-allocate at worst-case size n, fill in-place tracking a count, then call `truncate_array(idx, count)` to shrink the array to the actual number of elements retained.

**For single-accumulator functions (e.g., `std.foldl`):** The result is one `Value` that changes each iteration, not a growing array. Push `[elements..., func_val]` once. For the accumulator, push `[acc]` as a second single-element root frame before the loop; after each iteration, pop and re-push `[new_acc]`. Since it's just one `Value`, each pop+push is O(1).

**Open upvalues:** The linked-list walk (`open_upvalues`) is also moved outside the loop. The upvalue chain does not change during iteration, so it only needs to be collected once.

### New MemoryManager API

```rust
/// Set a single element of an existing GC-managed array.
/// Panics if index is out of bounds.
pub fn set_array_element(&mut self, idx: ArrayIndex, i: usize, val: Value);

/// Truncate a GC-managed array to new_len elements, dropping the rest.
/// Panics if new_len > current length.
pub fn truncate_array(&mut self, idx: ArrayIndex, new_len: usize);
```

### Scope

This fix applies to every std function that follows the per-iteration `push_external_roots` pattern. At the time of writing, confirmed affected functions include:

- `std.map`, `std.mapWithIndex`, `std.mapWithKey`, `std.mapKeys`
- `std.filter`
- `std.foldl`
- `std.sort` (key extraction loop)
- `std.setUnion`, `std.setInter`, `std.setDiff`
- `std.flatMap` (if present)

All call sites should be updated in this pass.

---

## Bottleneck 2: Per-Element Bytecode Chunk in `std.makeArray`

### Problem

`std.makeArray(n, f)` builds a lazy array where each element is evaluated as `f(i)` on demand. The current implementation represents each lazy element as a fully compiled bytecode program:

```rust
for i in 0..n {
    let mut element_chunk = chunk::Chunk::new("<makearray_element>");
    let fc_idx = element_chunk.add_constant(func_val);
    let ii_idx = element_chunk.add_constant(Value::Number(i as f64));
    element_chunk.write_opcode_u16(Opcode::LoadConst, fc_idx as u16, 0..0);
    element_chunk.write_opcode_u16(Opcode::LoadConst, ii_idx as u16, 0..0);
    element_chunk.write_opcode_u8_u8(Opcode::Call, 1, 0, 0..0);
    element_chunk.write_opcode(Opcode::Return, 0..0);
    let owned = element_chunk.into_owned();
    let func_alloc = self.memory_manager.allocate_function(None, 0, 0, owned);
    let thunk_alloc = self.memory_manager.allocate_thunk(func_alloc.index, Vec::new());
    elements.push(Value::Closure(thunk_alloc.index));
}
```

For `makeArray(2000, f)` this creates 2,000 `Chunk` objects, 2,000 `ManagedFunction` objects, and 2,000 `ManagedClosure` objects — 6,000 allocations — just to represent "call f with an integer index". This is pure overhead: two `Value`s per element is all that's needed.

### Fix: `ManagedNativeThunk`

Introduce a dedicated `ManagedNativeThunk` type stored in its own SlotMap in `MemoryManager`. A native thunk stores the function to call, the argument to pass, and a memoized result once forced.

**New type in `chunk.rs`:**
```rust
new_key_type! { pub struct NativeThunkIndex; }

// Value enum gets a new variant:
pub enum Value {
    // ... existing variants ...
    NativeThunk(NativeThunkIndex),
}
```

**New managed type in `memory_manager.rs`:**
```rust
pub struct ManagedNativeThunk {
    /// The function to call when forced
    pub func: Value,
    /// The argument to pass to func
    pub arg: Value,
    /// Cached result after first forcing (None = not yet forced)
    pub cached: Option<Value>,
    pub marked: Cell<bool>,
}
```

**New SlotMap and allocation method in `MemoryManager`:**
```rust
native_thunks: SlotMap<NativeThunkIndex, ManagedNativeThunk>,

pub fn allocate_native_thunk(&mut self, func: Value, arg: Value)
    -> AllocationResult<NativeThunkIndex>;
```

**`std.makeArray` becomes:**
```rust
for i in 0..sz {
    let thunk = self.memory_manager.allocate_native_thunk(
        func_val,
        Value::Number(i as f64),
    );
    elements.push(Value::NativeThunk(thunk.index));
}
```

**Forcing a `NativeThunk`:** Update the thunk-forcing path in `virtual_machine.rs` (in `force_thunk` and wherever `Value::Closure` is checked for `is_thunk`) to also handle `Value::NativeThunk(idx)`:

```rust
Value::NativeThunk(idx) => {
    if let Some(cached) = self.memory_manager.load_native_thunk(idx).cached {
        return Ok(cached);
    }
    let (func, arg) = {
        let t = self.memory_manager.load_native_thunk(idx);
        (t.func, t.arg)
    };
    let result = self.call_value_with_one_arg(func, arg)?;
    self.memory_manager.load_native_thunk_mut(idx).cached = Some(result);
    Ok(result)
}
```

**GC:** `ManagedNativeThunk` must be marked during GC traversal: mark `func`, `arg`, and `cached` (if Some). Add `native_thunks` to the GC mark/sweep loops.

---

## Bottleneck 3: `std.join` String Clone

### Problem

`std_join` (string mode) in `native.rs` collects all elements into a `Vec<String>` (cloning each), then calls `parts.join(&sep)`:

```rust
let mut parts: Vec<String> = Vec::with_capacity(elements.len());
for elem in &elements {
    match elem {
        Value::String(s_idx) => {
            parts.push(memory_manager.load_string(*s_idx).to_string()); // clone
        }
        ...
    }
}
let result = parts.join(&sep);
```

For 4000 strings this allocates a 4000-entry `Vec<String>` where every entry is a clone of an already-interned string.

### Fix: Single-pass with pre-allocated capacity

```rust
// Pass 1: compute total output length
let mut total_len = 0usize;
let mut non_null_count = 0usize;
for elem in &elements {
    match elem {
        Value::Null => {}
        Value::String(s_idx) => {
            total_len += memory_manager.load_string(*s_idx).len();
            non_null_count += 1;
        }
        _ => return Err(...),
    }
}
if non_null_count > 0 {
    total_len += sep.len() * (non_null_count - 1);
}

// Pass 2: build result in one allocation
let mut result = String::with_capacity(total_len);
let mut first = true;
for elem in &elements {
    if let Value::String(s_idx) = elem {
        if !first { result.push_str(&sep); }
        result.push_str(memory_manager.load_string(*s_idx));
        first = false;
    }
}
```

No intermediate `Vec<String>`, no per-element clone.

---

## Future Direction: Approach C — Unsafe Borrowed Root Slices

*Documented here for future consideration; not implemented in this pass.*

Add `push_root_borrow(roots: *const [Value], upvalue_roots: *const [UpvalueIndex])` to `MemoryManager` that stores raw pointers to caller-owned slices. GC reads through the pointers directly. Zero copying — the caller's Rust `Vec<Value>` is borrowed in place.

This requires:
- `unsafe` Rust at every call site with `# SAFETY` documentation
- The caller must guarantee the pointed-to slice stays valid and does not move for the duration of the borrow
- The `Vec<Value>` must not reallocate (i.e., no `push` calls) while borrowed

If profiling after Approach A shows GC root overhead is still significant (e.g., for very large arrays where even one root frame push is expensive), revisit this.

---

## Files Changed

| File | Changes |
|---|---|
| `src/chunk.rs` | Add `NativeThunkIndex` key type; add `Value::NativeThunk(NativeThunkIndex)` variant |
| `src/memory_manager.rs` | Add `ManagedNativeThunk`; add `native_thunks` SlotMap; add `allocate_native_thunk`, `set_array_element`, `truncate_array`; update GC mark/sweep for `native_thunks` and `Value::NativeThunk` |
| `src/virtual_machine.rs` | Restructure all std iteration loops to push-once pattern; update `force_thunk` for `Value::NativeThunk`; rewrite `std.makeArray` |
| `src/native.rs` | Rewrite `std_join` string mode with two-pass capacity pre-calculation |

---

## Success Criteria

- All existing tests pass (`bazel test //...`)
- `gen_big_object.jsonnet` benchmark time reduced (target: closer to GoJsonnet's 65ms from current 232ms)
- No regression on other benchmarks
- No new `unsafe` code introduced
