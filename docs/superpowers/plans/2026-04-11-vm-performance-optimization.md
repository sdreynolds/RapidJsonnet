# VM Performance Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate O(n²) GC root rebuilding in std iteration functions, remove per-element bytecode chunk allocation in std.makeArray, and fix std.join string cloning.

**Architecture:** Three independent but sequentially dependent changes: (1) add `NativeThunkIndex`/`Value::NativeThunk`/`ManagedNativeThunk` types as infrastructure; (2) update GC and forcing logic to handle the new type; (3) rewrite `std.makeArray` and all std iteration function loops to use the new efficient rooting pattern.

**Tech Stack:** Rust, Bazel (rules_rust), slotmap crate, `bazel test //...` for verification.

---

## File Map

| File | Changes |
|---|---|
| `src/chunk.rs` | Add `NativeThunkIndex` type alias; add `Value::NativeThunk(NativeThunkIndex)` variant; update `type_name`, `Hash`, `Display` |
| `src/memory_manager.rs` | Add `ManagedNativeThunk` struct; add `native_thunks` SlotMap field; add `allocate_native_thunk`, `load_native_thunk`, `load_native_thunk_mut`, `set_array_element`, `truncate_array`; update GC mark+sweep |
| `src/virtual_machine.rs` | Update `force_value` for `Value::NativeThunk`; rewrite `std.makeArray` (two call sites); refactor GC rooting in `std.map`, `std.filter`, `std.mapWithIndex`, `std.foldl`, `std.foldr`, `std.flatMap`, sort key loop, `std.setUnion/setInter/setDiff`, `std.mapWithKey`, `std.mapKeys` |
| `src/native.rs` | Rewrite `std_join` string mode with two-pass capacity pre-calculation |

---

## Task 1: Add NativeThunkIndex and Value::NativeThunk to chunk.rs

**Files:**
- Modify: `src/chunk.rs`

- [ ] **Step 1: Add the NativeThunkIndex type alias after line 21**

In `src/chunk.rs`, line 21 currently reads `pub type BinaryIndex = DefaultKey;`. Add the new type directly after it:

```rust
pub type BinaryIndex = DefaultKey;
pub type NativeThunkIndex = DefaultKey;
```

- [ ] **Step 2: Add the NativeThunk variant to the Value enum**

The `Value` enum is at approximately line 1226. Add `NativeThunk(NativeThunkIndex)` after the `Binary` variant:

```rust
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(StringIndex),
    Object(ObjectIndex),
    Array(ArrayIndex),
    Function(FunctionIndex),
    Closure(ClosureIndex),
    Import(ImportIndex),
    Binary(BinaryIndex),
    NativeThunk(NativeThunkIndex),
    NativeFunction(NativeFuncId),
    /// Sentinel for function parameters not provided by the caller.
    /// Never observable from Jsonnet code.
    Uninitialized,
}
```

- [ ] **Step 3: Update type_name() to handle NativeThunk**

In the `type_name()` method, add `Value::NativeThunk(_) => "function"` to the match:

```rust
Value::Function(_) | Value::Closure(_) | Value::NativeFunction(_) | Value::NativeThunk(_) => "function",
```

- [ ] **Step 4: Update the Hash impl to handle NativeThunk**

In the `impl std::hash::Hash for Value` block, add a new arm after the `Binary` arm (which uses `9u8`). Use `12u8` (since `11u8` is Uninitialized):

```rust
Value::NativeThunk(key) => {
    12u8.hash(state);
    key.hash(state);
}
```

- [ ] **Step 5: Update the Display impl to handle NativeThunk**

Find `impl std::fmt::Display for Value` and add a NativeThunk arm. It should never be displayed in normal output, but we need the match to be exhaustive. Add it alongside other non-displayable types:

```rust
Value::NativeThunk(_) => write!(f, "<thunk>"),
```

- [ ] **Step 6: Build to find and fix any remaining exhaustive match errors**

```bash
bazel build //... 2>&1 | grep "error\[E" | head -30
```

Fix any non-exhaustive match errors by adding `Value::NativeThunk(_) => ...` arms following the same pattern as `Value::Binary(_)` or `Value::Uninitialized` in those locations.

- [ ] **Step 7: Run tests**

```bash
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/chunk.rs
git commit -m "feat: add NativeThunkIndex and Value::NativeThunk variant"
```

---

## Task 2: Add ManagedNativeThunk and MemoryManager API

**Files:**
- Modify: `src/memory_manager.rs`

- [ ] **Step 1: Update the import in memory_manager.rs to include NativeThunkIndex**

At the top of `src/memory_manager.rs`, the existing import reads:
```rust
use chunk::{
    ArrayIndex, BinaryIndex, ClosureIndex, FieldVisibility, FunctionIndex, ImportIndex,
    ObjectIndex, OwnedChunk, SpanRunLength, StringIndex, UpvalueIndex, Value,
};
```

Add `NativeThunkIndex` to this import:
```rust
use chunk::{
    ArrayIndex, BinaryIndex, ClosureIndex, FieldVisibility, FunctionIndex, ImportIndex,
    NativeThunkIndex, ObjectIndex, OwnedChunk, SpanRunLength, StringIndex, UpvalueIndex, Value,
};
```

- [ ] **Step 2: Add the ManagedNativeThunk struct**

Add this struct after the `ManagedBinary` struct (around line 374, after the `ManagedBinary` impl block):

```rust
/// A lightweight native thunk: a (func, arg) pair evaluated lazily without bytecode.
/// Used by std.makeArray to avoid per-element Chunk allocation.
pub struct ManagedNativeThunk {
    /// The function to call when forced
    pub func: Value,
    /// The argument to pass to func
    pub arg: Value,
    /// Cached result after first forcing (None = not yet forced)
    pub cached: Option<Value>,
    pub(crate) marked: Cell<bool>,
}

impl ManagedNativeThunk {
    pub fn new(func: Value, arg: Value) -> Self {
        Self {
            func,
            arg,
            cached: None,
            marked: Cell::new(false),
        }
    }

    fn size(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}
```

- [ ] **Step 3: Add native_thunks field to MemoryManager struct**

In the `MemoryManager` struct definition (around line 377), add the field after `binaries`:

```rust
/// Collection of Native Thunks (for std.makeArray lazy elements)
native_thunks: SlotMap<NativeThunkIndex, ManagedNativeThunk>,
```

- [ ] **Step 4: Initialize native_thunks in MemoryManager::new()**

In the `new()` method, add initialization after `binaries: SlotMap::with_key()`:

```rust
native_thunks: SlotMap::with_key(),
```

- [ ] **Step 5: Add allocate_native_thunk, load_native_thunk, load_native_thunk_mut methods**

Add these methods to the `impl MemoryManager` block, near the other `allocate_*` methods:

```rust
pub fn allocate_native_thunk(&mut self, func: Value, arg: Value) -> AllocationResult<NativeThunkIndex> {
    let thunk = ManagedNativeThunk::new(func, arg);
    self.allocated_bytes += thunk.size();
    let index = self.native_thunks.insert(thunk);
    AllocationResult {
        should_garbage_collect: self.should_collect(),
        index,
    }
}

pub fn load_native_thunk(&self, key: NativeThunkIndex) -> &ManagedNativeThunk {
    self.native_thunks.get(key).expect("NativeThunk not found")
}

pub fn load_native_thunk_mut(&mut self, key: NativeThunkIndex) -> &mut ManagedNativeThunk {
    self.native_thunks.get_mut(key).expect("NativeThunk not found")
}
```

- [ ] **Step 6: Add set_array_element and truncate_array methods**

Add these two methods to `impl MemoryManager`:

```rust
/// Set a single element of an existing GC-managed array in-place.
/// Used by std iteration functions to fill pre-allocated result arrays without
/// rebuilding GC root lists on every iteration.
/// Panics if index i is out of bounds.
pub fn set_array_element(&mut self, idx: ArrayIndex, i: usize, val: Value) {
    let arr = self.arrays.get_mut(idx).expect("Array not found");
    arr.elements[i] = val;
}

/// Truncate a GC-managed array to new_len elements.
/// Used by std.filter (and similar) to shrink a worst-case-sized result array
/// to the actual number of elements retained.
/// Panics if new_len > current length.
pub fn truncate_array(&mut self, idx: ArrayIndex, new_len: usize) {
    let arr = self.arrays.get_mut(idx).expect("Array not found");
    assert!(new_len <= arr.elements.len(), "truncate_array: new_len exceeds current length");
    arr.elements.truncate(new_len);
}
```

- [ ] **Step 7: Build to catch any compilation errors**

```bash
bazel build //... 2>&1 | grep "error" | head -20
```

Expected: compiles cleanly.

- [ ] **Step 8: Run tests**

```bash
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/memory_manager.rs
git commit -m "feat: add ManagedNativeThunk, set_array_element, truncate_array to MemoryManager"
```

---

## Task 3: Update GC mark+sweep for NativeThunk

**Files:**
- Modify: `src/memory_manager.rs`

- [ ] **Step 1: Add NativeThunk arm to the GC mark phase**

In `run_garbage_collect`, the mark phase `while let Some(head) = values.pop_front()` match block currently ends with `_ => continue`. Add a new arm before `_ => continue`:

```rust
Value::NativeThunk(thunk_index) => {
    if let Some(thunk) = self.native_thunks.get_mut(thunk_index) {
        if !thunk.marked.get() {
            thunk.marked.set(true);
            // Mark the function and argument
            values.push_back(thunk.func);
            values.push_back(thunk.arg);
            // Mark the cached result if present
            if let Some(cached) = thunk.cached {
                values.push_back(cached);
            }
        }
    }
}
```

- [ ] **Step 2: Add NativeThunk sweep loop in the sweep phase**

After the `binaries_to_delete` sweep loop and before the `gc_threshold` update at line 1071, add:

```rust
let mut native_thunks_to_delete: Vec<NativeThunkIndex> = Vec::new();
for (thunk_idx, thunk) in self.native_thunks.iter_mut() {
    if thunk.marked.get() {
        thunk.marked.set(false);
    } else {
        native_thunks_to_delete.push(thunk_idx);
        self.allocated_bytes = self.allocated_bytes.saturating_sub(thunk.size());
    }
}

for thunk_idx in native_thunks_to_delete {
    self.native_thunks.remove(thunk_idx);
}
```

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/memory_manager.rs
git commit -m "feat: add NativeThunk GC mark and sweep to MemoryManager"
```

---

## Task 4: Update force_value to handle Value::NativeThunk

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Add NativeThunk arm to force_value**

In `src/virtual_machine.rs`, the `force_value` method is at approximately line 1373. It currently ends with:

```rust
            _ => Ok(val),
        }
    }
```

Add a `Value::NativeThunk` arm before the `_ => Ok(val)` fallthrough:

```rust
            Value::NativeThunk(thunk_idx) => {
                // Return cached result if already forced
                if let Some(cached) = self.memory_manager.load_native_thunk(thunk_idx).cached {
                    return Ok(cached);
                }
                // Load func and arg (copy out to avoid borrow)
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
```

- [ ] **Step 2: Build to check for any remaining non-exhaustive match errors**

```bash
bazel build //... 2>&1 | grep "error" | head -20
```

If there are exhaustive-match errors on `Value` in virtual_machine.rs (unlikely since most matches have `_` arms), fix them by adding `Value::NativeThunk(_) => ...` arms following the same pattern as `Value::Uninitialized` or `Value::Binary`.

- [ ] **Step 3: Run tests**

```bash
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "feat: add NativeThunk forcing path in force_value"
```

---

## Task 5: Rewrite std.makeArray to use NativeThunk

**Files:**
- Modify: `src/virtual_machine.rs`

There are **two** call sites where `std.makeArray` is handled — one in `call_native_checked` (around line 666) and one in the main `interpret_until` dispatch loop (around line 3044). Both must be updated.

- [ ] **Step 1: Replace the first makeArray call site (~line 666)**

Find the block starting with `} else if id == chunk::NativeFuncId::MakeArray && args.len() == 2 {`. Replace the entire body of this arm (from the opening brace through `Ok(Value::Array(alloc.index))` and the closing brace) with:

```rust
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
                let thunk_alloc = self.memory_manager.allocate_native_thunk(
                    func_val,
                    Value::Number(i as f64),
                );
                should_gc |= thunk_alloc.should_garbage_collect;
                elements.push(Value::NativeThunk(thunk_alloc.index));
            }
            self.memory_manager
                .push_external_roots(elements.clone(), Vec::new());
            let alloc = self.memory_manager.allocate_array(elements);
            self.memory_manager.pop_external_roots();
            if should_gc || alloc.should_garbage_collect {
                self.run_garbage_collection();
            }
            Ok(Value::Array(alloc.index))
```

- [ ] **Step 2: Replace the second makeArray call site (~line 3044)**

Find the second block `if func_id == chunk::NativeFuncId::MakeArray {` inside the main dispatch loop. Replace its body (everything up through and including `self.push(Value::Array(array_alloc.index))?;` and `continue;`) with:

```rust
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
                            let thunk_alloc = self.memory_manager.allocate_native_thunk(
                                func_val,
                                Value::Number(i as f64),
                            );
                            should_gc |= thunk_alloc.should_garbage_collect;
                            elements.push(Value::NativeThunk(thunk_alloc.index));
                        }
                        self.memory_manager
                            .push_external_roots(elements.clone(), Vec::new());
                        let array_alloc = self.memory_manager.allocate_array(elements);
                        self.memory_manager.pop_external_roots();
                        if should_gc || array_alloc.should_garbage_collect {
                            self.run_garbage_collection();
                        }
                        self.push(Value::Array(array_alloc.index))?;
                        continue;
                    }
```

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "perf: rewrite std.makeArray to use NativeThunk instead of per-element bytecode chunks"
```

---

## Task 6: Fix GC rooting in std.map and std.mapWithIndex

**Files:**
- Modify: `src/virtual_machine.rs`

Both functions build a new `Vec` of all elements + all results on every loop iteration. Replace both with the push-once + fill-in-place pattern.

- [ ] **Step 1: Replace the std.map loop body (~line 3482)**

Find `if func_id == chunk::NativeFuncId::Map {`. Replace the entire handler (from `{` through `continue;`) with:

```rust
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
                        let n = elements.len();

                        // Collect open upvalue roots once — the chain doesn't change mid-loop
                        let mut open_upvalue_roots = Vec::new();
                        let mut upvalue = self.open_upvalues;
                        while let Some(uv_idx) = upvalue {
                            open_upvalue_roots.push(uv_idx);
                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                        }

                        // Push input elements + func as stable roots once before the loop
                        let mut input_roots = elements.clone();
                        input_roots.push(func_val);
                        self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                        // Pre-allocate result array so GC can trace results as they're written
                        let result_arr_idx = self.memory_manager
                            .allocate_array(vec![Value::Null; n])
                            .index;
                        self.memory_manager.push_external_roots(
                            vec![Value::Array(result_arr_idx)],
                            vec![],
                        );

                        // Fill in-place — no per-iteration Vec allocation
                        for i in 0..n {
                            let elem = elements[i];
                            let result = self.call_value_with_one_arg(func_val, elem)?;
                            self.memory_manager.set_array_element(result_arr_idx, i, result);
                        }

                        self.memory_manager.pop_external_roots(); // result array frame
                        self.memory_manager.pop_external_roots(); // input elements frame
                        self.push(Value::Array(result_arr_idx))?;
                        continue;
                    }
```

- [ ] **Step 2: Replace the std.mapWithIndex loop body (~line 3710)**

Find `if func_id == chunk::NativeFuncId::MapWithIndex {`. Replace the entire handler with:

```rust
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
                        let n = elements.len();

                        let mut open_upvalue_roots = Vec::new();
                        let mut upvalue = self.open_upvalues;
                        while let Some(uv_idx) = upvalue {
                            open_upvalue_roots.push(uv_idx);
                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                        }

                        let mut input_roots = elements.clone();
                        input_roots.push(func_val);
                        self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                        let result_arr_idx = self.memory_manager
                            .allocate_array(vec![Value::Null; n])
                            .index;
                        self.memory_manager.push_external_roots(
                            vec![Value::Array(result_arr_idx)],
                            vec![],
                        );

                        for i in 0..n {
                            let elem = elements[i];
                            let result = self.call_value_with_two_args(
                                func_val,
                                Value::Number(i as f64),
                                elem,
                            )?;
                            self.memory_manager.set_array_element(result_arr_idx, i, result);
                        }

                        self.memory_manager.pop_external_roots();
                        self.memory_manager.pop_external_roots();
                        self.push(Value::Array(result_arr_idx))?;
                        continue;
                    }
```

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "perf: fix O(n²) GC rooting in std.map and std.mapWithIndex"
```

---

## Task 7: Fix GC rooting in std.filter and std.flatMap

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Replace the std.filter loop body (~line 3523)**

Find `if func_id == chunk::NativeFuncId::Filter {`. Replace the entire handler with:

```rust
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
                        let n = elements.len();

                        let mut open_upvalue_roots = Vec::new();
                        let mut upvalue = self.open_upvalues;
                        while let Some(uv_idx) = upvalue {
                            open_upvalue_roots.push(uv_idx);
                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                        }

                        let mut input_roots = elements.clone();
                        input_roots.push(func_val);
                        self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                        // Pre-allocate at worst-case size; truncate after the loop
                        let result_arr_idx = self.memory_manager
                            .allocate_array(vec![Value::Null; n])
                            .index;
                        self.memory_manager.push_external_roots(
                            vec![Value::Array(result_arr_idx)],
                            vec![],
                        );

                        let mut count = 0usize;
                        for i in 0..n {
                            let elem = elements[i];
                            let passes = self.call_value_with_one_arg(func_val, elem)?;
                            match passes {
                                Value::Boolean(true) => {
                                    self.memory_manager.set_array_element(result_arr_idx, count, elem);
                                    count += 1;
                                }
                                Value::Boolean(false) => {}
                                _ => {
                                    self.memory_manager.pop_external_roots();
                                    self.memory_manager.pop_external_roots();
                                    return Err(RuntimeError::new(
                                        self.get_current_span(),
                                        "std.filter: predicate must return boolean".to_string(),
                                        self.current_chunk().source_id.to_string(),
                                    ));
                                }
                            }
                        }

                        self.memory_manager.truncate_array(result_arr_idx, count);
                        self.memory_manager.pop_external_roots();
                        self.memory_manager.pop_external_roots();
                        self.push(Value::Array(result_arr_idx))?;
                        continue;
                    }
```

- [ ] **Step 2: Replace the std.flatMap array-branch loop body (~line 3612)**

Find `if func_id == chunk::NativeFuncId::FlatMap {` and within it the `Value::Array(arr_idx)` match arm. Replace just the array branch body (the `for &elem in &elements` loop through `let alloc = self.memory_manager.allocate_array(results)` and the push) with:

```rust
                            Value::Array(arr_idx) => {
                                let elements =
                                    self.memory_manager.load_array(arr_idx).elements.clone();

                                let mut open_upvalue_roots = Vec::new();
                                let mut upvalue = self.open_upvalues;
                                while let Some(uv_idx) = upvalue {
                                    open_upvalue_roots.push(uv_idx);
                                    upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                                }

                                let mut input_roots = elements.clone();
                                input_roots.push(func_val);
                                self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                                // Collect into a Rust Vec; root it as a GC array immediately after
                                // each sub-result to avoid accumulation without GC visibility
                                let mut results: Vec<Value> = Vec::new();
                                for &elem in &elements {
                                    let sub = self.call_value_with_one_arg(func_val, elem)?;
                                    match sub {
                                        Value::Array(sub_idx) => {
                                            let sub_elems = self
                                                .memory_manager
                                                .load_array(sub_idx)
                                                .elements
                                                .clone();
                                            results.extend(sub_elems);
                                        }
                                        _ => {
                                            self.memory_manager.pop_external_roots();
                                            return Err(RuntimeError::new(
                                                self.get_current_span(),
                                                "std.flatMap: function must return array for array input"
                                                    .to_string(),
                                                self.current_chunk().source_id.to_string(),
                                            ));
                                        }
                                    }
                                }

                                self.memory_manager.pop_external_roots();
                                // Root results while allocating the final array
                                self.memory_manager.push_external_roots(results.clone(), vec![]);
                                let alloc = self.memory_manager.allocate_array(results);
                                self.memory_manager.pop_external_roots();
                                self.push(Value::Array(alloc.index))?;
                            }
```

Note: flatMap string-branch (`Value::String`) is lower priority and can be left as-is for now — the string chars approach doesn't accumulate large results the same way.

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "perf: fix O(n²) GC rooting in std.filter and std.flatMap"
```

---

## Task 8: Fix GC rooting in std.foldl and std.foldr

**Files:**
- Modify: `src/virtual_machine.rs`

Both functions accumulate a single mutable `acc` value. The fix: push elements+func once, push acc as a single-element frame, pop+re-push acc each iteration (O(1) per iteration).

- [ ] **Step 1: Replace the std.foldl loop body (~line 3576)**

Find `if func_id == chunk::NativeFuncId::Foldl {`. Replace the entire handler with:

```rust
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

                        let mut open_upvalue_roots = Vec::new();
                        let mut upvalue = self.open_upvalues;
                        while let Some(uv_idx) = upvalue {
                            open_upvalue_roots.push(uv_idx);
                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                        }

                        // Push elements + func once as stable roots
                        let mut input_roots = elements.clone();
                        input_roots.push(func_val);
                        self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                        // Push initial acc as a single-element frame; update it each iteration
                        self.memory_manager.push_external_roots(vec![acc], vec![]);

                        for &elem in &elements {
                            let new_acc = self.call_value_with_two_args(func_val, acc, elem)?;
                            // O(1): pop single-element frame, push new acc
                            self.memory_manager.pop_external_roots();
                            acc = new_acc;
                            self.memory_manager.push_external_roots(vec![acc], vec![]);
                        }

                        self.memory_manager.pop_external_roots(); // acc frame
                        self.memory_manager.pop_external_roots(); // elements+func frame
                        self.push(acc)?;
                        continue;
                    }
```

- [ ] **Step 2: Replace the std.foldr loop body (~line 3752)**

Find `if func_id == chunk::NativeFuncId::Foldr {`. Replace the entire handler with:

```rust
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

                        let mut open_upvalue_roots = Vec::new();
                        let mut upvalue = self.open_upvalues;
                        while let Some(uv_idx) = upvalue {
                            open_upvalue_roots.push(uv_idx);
                            upvalue = self.memory_manager.load_upvalue(uv_idx).next;
                        }

                        let mut input_roots = elements.clone();
                        input_roots.push(func_val);
                        self.memory_manager.push_external_roots(input_roots, open_upvalue_roots);

                        self.memory_manager.push_external_roots(vec![acc], vec![]);

                        for &elem in elements.iter().rev() {
                            let new_acc = self.call_value_with_two_args(func_val, elem, acc)?;
                            self.memory_manager.pop_external_roots();
                            acc = new_acc;
                            self.memory_manager.push_external_roots(vec![acc], vec![]);
                        }

                        self.memory_manager.pop_external_roots();
                        self.memory_manager.pop_external_roots();
                        self.push(acc)?;
                        continue;
                    }
```

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "perf: fix O(n²) GC rooting in std.foldl and std.foldr"
```

---

## Task 9: Fix GC rooting in sort, set operations, mapWithKey, and mapKeys

**Files:**
- Modify: `src/virtual_machine.rs`

This task fixes the remaining iteration functions. Each follows the same pattern as Tasks 6-8. The functions are: sort key-extraction loop, setUnion/setInter/setDiff, mapWithKey, and mapKeys.

- [ ] **Step 1: Search for all remaining per-iteration push_external_roots patterns in virtual_machine.rs**

```bash
grep -n "extend_from_slice\|push_external_roots" src/virtual_machine.rs | grep -v "^.*//.*" | head -80
```

Identify every remaining loop that has `extend_from_slice(&elements)` or `extend_from_slice(&results)` inside it (i.e., inside a for loop body). These are the remaining O(n²) sites.

- [ ] **Step 2: Fix each remaining site using the pattern that matches its return type**

For each remaining call site from Step 1, apply the pattern that matches its shape:

**Array-in / array-out functions (sort key extraction, setUnion, setInter, setDiff):**
Use the same pattern as Task 6. The sort key extraction loop produces an array of sort keys — pre-allocate `vec![Value::Null; n]`, fill in-place with `set_array_element`. The set operation loops (setUnion/setInter/setDiff) work over two input arrays producing one output array — push both input arrays + func once, pre-allocate result at worst-case size (len(a) + len(b)), fill in-place, truncate at end.

**Object-in / object-out functions (mapWithKey, mapKeys):**
These build a `HashMap<StringIndex, ObjectField>` as their result, not an array, so `set_array_element` does not apply. For these: (a) move the `open_upvalue_roots` linked-list walk outside the loop body (it is currently inside), and (b) push the full input field data + `func_val` as a single root frame before the loop; pop it after. The accumulating `new_properties` HashMap contains Values that are GC-visible only because they are freshly returned from `call_value_with_one_arg` (on the VM stack); root the accumulated properties by collecting their values into a `Vec<Value>` that is pushed as a second external root frame before the loop and updated per-iteration with `pop_external_roots` + `push_external_roots(accumulated_values, vec![])`. Since object fields are typically small in number compared to benchmark array sizes, the per-iteration update here is not the primary bottleneck, but the upvalue hoist alone eliminates the linked-list walk cost.

- [ ] **Step 3: Build and run tests after each changed function**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass after each change.

- [ ] **Step 4: Commit after all remaining functions are fixed**

```bash
git add src/virtual_machine.rs
git commit -m "perf: fix O(n²) GC rooting in sort, set operations, mapWithKey, and mapKeys"
```

---

## Task 10: Rewrite std_join string mode in native.rs

**Files:**
- Modify: `src/native.rs`

- [ ] **Step 1: Locate the std_join string mode**

The function `fn std_join` is at approximately line 1315 in `src/native.rs`. The string separator branch starts with `Value::String(sep_idx) => {`.

- [ ] **Step 2: Replace the string separator branch**

Replace the entire `Value::String(sep_idx) => { ... }` branch body (everything from the `let sep = ...` line through `Ok(Value::String(alloc.index))`) with:

```rust
        Value::String(sep_idx) => {
            let sep = memory_manager.load_string(sep_idx).to_string();
            let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();

            // Validate all elements and count total output length in one pass
            let mut total_len = 0usize;
            let mut non_null_count = 0usize;
            for elem in &elements {
                match elem {
                    Value::Null => continue,
                    Value::String(s_idx) => {
                        total_len += memory_manager.load_string(*s_idx).len();
                        non_null_count += 1;
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "std.join() with string separator requires array of strings"
                                .to_string(),
                            source_id,
                        ));
                    }
                }
            }
            if non_null_count > 1 {
                total_len += sep.len() * (non_null_count - 1);
            }

            // Build result in a single allocation — no intermediate Vec<String>
            let mut result = String::with_capacity(total_len);
            let mut first = true;
            for elem in &elements {
                if let Value::String(s_idx) = elem {
                    if !first {
                        result.push_str(&sep);
                    }
                    result.push_str(memory_manager.load_string(*s_idx));
                    first = false;
                }
            }

            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
```

- [ ] **Step 3: Build and run tests**

```bash
bazel build //... 2>&1 | grep "error" | head -20
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/native.rs
git commit -m "perf: rewrite std.join string mode with two-pass capacity pre-calculation"
```

---

## Task 11: Run full test suite and verify benchmark improvement

**Files:** None changed

- [ ] **Step 1: Run the complete test suite**

```bash
bazel test //...
```

Expected: all tests pass with zero failures.

- [ ] **Step 2: Run the benchmark**

```bash
bazel run //benchmarks:benchmark
```

Compare `benchmark-results/gen_big_object.jsonnet-results.md` against the pre-optimization baseline:

| Command | Mean before | Mean after (target) |
|---|---|---|
| RapidJsonnet | 232.3 ± 4.8 ms | < 150 ms |
| GoJsonnet | 65.8 ± 7.5 ms | 65.8 ± 7.5 ms (unchanged) |

- [ ] **Step 3: Check for regressions on other benchmarks**

Review all files in `benchmark-results/` for any benchmark that regressed (increased time). If a regression is found, investigate and fix before proceeding.

- [ ] **Step 4: Final commit if any last-minute fixes were needed**

```bash
git add .
git commit -m "perf: final vm optimization pass — verified benchmarks"
```

---

## Summary of Changes

| Bottleneck | Fix | Expected Impact |
|---|---|---|
| O(n²) GC root rebuilding in 10+ std functions | Push-once + fill-in-place pattern | Largest win for large array operations |
| Per-element bytecode chunk in std.makeArray | `ManagedNativeThunk` replaces 3 allocations with 1 | Significant for makeArray-heavy benchmarks |
| std.join string cloning | Two-pass with `String::with_capacity` | Minor but measurable for large string joins |
