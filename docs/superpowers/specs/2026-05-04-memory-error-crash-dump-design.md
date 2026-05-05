# Memory Error Handling and Crash Dump Design

**Date:** 2026-05-04
**Status:** Approved

## Overview

Replace panicking `.expect()` calls in `memory_manager.rs` with `Result`-returning methods that propagate a new `MemoryError` type up through the VM to a single crash dump site at the public `interpret()` boundary. When an invalid SlotMap key is detected, a plain-text crash dump file is written to the current working directory before returning an error to the caller.

These errors represent interpreter bugs (GC or compiler defects), not user Jsonnet mistakes. The crash dump audience is both the developer (debugging) and users filing GitHub issues (the dump is safe to attach publicly).

## Error Types

### `MemoryError` (in `memory_manager.rs`)

```rust
pub struct MemoryError {
    pub key_type: &'static str,  // "Object", "String", "Array", etc.
    pub key_debug: String,        // format!("{:?}", key)
}
```

No span — span is captured at the `interpret()` boundary via `instruction_start_ip`.

### `VmError` (in `virtual_machine.rs`)

```rust
pub enum VmError {
    Runtime(RuntimeError),
    Memory(MemoryError),
}
impl From<RuntimeError> for VmError { ... }
impl From<MemoryError> for VmError { ... }
```

All internal/private VM methods return `Result<T, VmError>`. The four public methods that call `interpret_until` (`interpret`, `force_field_thunk`, `call_test_closure`, `set_ext_var_code`) stay `Result<T, RuntimeError>` — conversion happens at those boundaries via a shared private helper.

## Memory Manager Changes

### `MemoryStats` struct (new)

```rust
pub struct MemoryStats {
    pub allocated_bytes: usize,
    pub gc_threshold: usize,
    pub strings: usize,
    pub objects: usize,
    pub arrays: usize,
    pub functions: usize,
    pub closures: usize,
    pub upvalues: usize,
    pub imports: usize,
    pub binaries: usize,
    pub native_thunks: usize,
}
```

Add `pub fn diagnostic_stats(&self) -> MemoryStats` alongside the existing `stats()`.

### Load method signature changes (17 methods)

All `load_*` methods change from panicking to `Result`-returning:

| Method | Before | After |
|---|---|---|
| `load_object` | `&ManagedObject` | `Result<&ManagedObject, MemoryError>` |
| `load_string` | `&str` | `Result<&str, MemoryError>` |
| `load_array` | `&ManagedArray` | `Result<&ManagedArray, MemoryError>` |
| `load_array_mut` | `&mut ManagedArray` | `Result<&mut ManagedArray, MemoryError>` |
| `set_array_element` | `()` (panics) | `Result<(), MemoryError>` |
| `truncate_array` | `()` (panics) | `Result<(), MemoryError>` |
| `load_function` | `&ManagedFunction` | `Result<&ManagedFunction, MemoryError>` |
| `load_function_mut` | `&mut ManagedFunction` | `Result<&mut ManagedFunction, MemoryError>` |
| `load_closure` | `&ManagedClosure` | `Result<&ManagedClosure, MemoryError>` |
| `load_upvalue` | `&ManagedUpvalue` | `Result<&ManagedUpvalue, MemoryError>` |
| `load_upvalue_mut` | `&mut ManagedUpvalue` | `Result<&mut ManagedUpvalue, MemoryError>` |
| `load_import` | `&ManagedImport` | `Result<&ManagedImport, MemoryError>` |
| `load_import_mut` | `&mut ManagedImport` | `Result<&mut ManagedImport, MemoryError>` |
| `load_binary` | `&ManagedBinary` | `Result<&ManagedBinary, MemoryError>` |
| `load_binary_mut` | `&mut ManagedBinary` | `Result<&mut ManagedBinary, MemoryError>` |
| `load_native_thunk` | `&ManagedNativeThunk` | `Result<&ManagedNativeThunk, MemoryError>` |
| `load_native_thunk_mut` | `&mut ManagedNativeThunk` | `Result<&mut ManagedNativeThunk, MemoryError>` |

`get_object_mut` already returns `Option` — no change.

`collect_object_fields_chain` calls `load_object` internally and changes to return `Result<Vec<(StringIndex, Value, FieldVisibility)>, MemoryError>`.

Pattern for each conversion:
```rust
pub fn load_object(&self, key: ObjectIndex) -> Result<&ManagedObject, MemoryError> {
    self.objects.get(key).ok_or_else(|| MemoryError {
        key_type: "Object",
        key_debug: format!("{:?}", key),
    })
}
```

## VM Method Signature Migration

Change all internal VM helper methods from `Result<T, RuntimeError>` to `Result<T, VmError>`. With `From<RuntimeError> for VmError` and `From<MemoryError> for VmError` implemented, existing `?` on `RuntimeError` and new `?` on `MemoryError` both work without explicit conversion.

The compiler drives this mechanically: change `interpret_until` first, then follow each compile error upstream.

### Public boundary conversion helper

```rust
fn finalize_vm_error(&mut self, e: VmError) -> RuntimeError {
    match e {
        VmError::Runtime(re) => self.build_stack_trace(re),
        VmError::Memory(me) => {
            let path = self.write_crash_dump(&me);
            eprintln!(
                "rapidjsonnet: interpreter bug — crash dump written to {}",
                path.display()
            );
            RuntimeError::new(
                0..0,
                format!("internal interpreter error (see {})", path.display()),
                String::new(),
            )
        }
    }
}
```

`interpret()` after migration:
```rust
pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
    self.interpret_until(0)
        .map_err(|e| self.finalize_vm_error(e))
}
```

`native.rs` functions are called from the VM and do not call `load_*` directly — they stay `Result<Value, RuntimeError>` and the VM converts via `?`.

## Crash Dump Writer

Private method on `VirtualMachine`:

```rust
fn write_crash_dump(&self, err: &MemoryError) -> PathBuf
```

### File naming

`rapidjsonnet-crash-YYYY-MM-DDTHH-MM-SS.txt` in the current working directory, using `std::time::SystemTime`. No external crates.

### Plain text format

```
=== RapidJsonnet Crash Report ===
Timestamp: 2026-05-04T12:34:56Z
Platform: linux x86_64

This is an interpreter bug. Please file an issue and attach this file.

--- Internal Error ---
Corrupted memory slot
  Key type: Object
  Key:      GenerationalIndex { idx: 42, version: 1 }

--- Execution Location ---
  Source file:        /home/scott/example/test.jsonnet
  Instruction offset: 142

  Source context:
     13 | local compute = function(x)
     14 |   local result = {
  >> 15 |     value: self.helper(x),
     16 |   };
     17 |   result

--- Call Stack ---
  [0] <top-level> @ test.jsonnet
  [1] compute @ test.jsonnet
  [2] <thunk> @ test.jsonnet

--- VM State ---
  Stack depth:    12
  Active frames:   3
  Open upvalues:   2

--- Memory Statistics ---
  Allocated:     2,097,152 bytes (2.0 MB)
  GC threshold:  4,194,304 bytes (4.0 MB)
  Strings:         142
  Objects:          38
  Arrays:           12
  Functions:         8
  Closures:         15
  Upvalues:          6
  Imports:           2
  Binaries:          0
  Native thunks:     4
```

### Source snippet

Resolve the current chunk by calling `load_closure(current_frame.closure)` then `load_function(closure.function)` — both now return `Result`. If either fails (e.g., a corrupted Closure key caused the crash), omit the entire Execution Location section. If the chunk is available, read the source file from disk using `source_id` and resolve the byte span via `chunk.get_span(self.instruction_start_ip)`. Convert the byte offset to line/column for display. If the file cannot be read, omit the source context lines but still show file path and instruction offset.

### Call stack

Walk `self.frames[0..self.frame_count]`. For each frame, try to resolve the closure and function names via `load_closure` / `load_function`. If either fails (the corrupted key type was Closure or Function), print `<unavailable>` for that frame and continue. The writer never panics — every `load_*` call uses `match` with a fallback string.

## Testing

### Unit tests in `memory_manager.rs`

Verify `load_*` returns `Err(MemoryError)` after a key is removed. Extend the existing `garbage_collection` test:

```rust
#[test]
fn load_after_gc_returns_error() {
    let mut manager = MemoryManager::new();
    let idx = manager.allocate_object().index;
    manager.run_garbage_collect(vec![], vec![]);
    assert!(manager.load_object(idx).is_err());
}
```

### Integration test in `virtual_machine.rs`

Confirm `interpret()` returns `Err` (not a panic) when a memory error occurs, and that a crash dump file was written to disk. Clean up the file after the test.

### End-to-end

Run an existing passing Jsonnet end-to-end test to confirm the signature migration did not break normal execution paths.

## Out of Scope

- Other panic sites in the codebase (stack overflow, assertion failures, etc.)
- Global panic hook
- Structured (JSON/TOML) dump format
- Crash reporting services (Sentry, etc.)
