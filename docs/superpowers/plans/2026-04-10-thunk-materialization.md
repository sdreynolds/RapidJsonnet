# Thunk Materialization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the "thunk is not materialized" runtime error by centralizing thunk forcing in the VM, ensuring native functions receive forced data, and supporting standalone execution of top-level parameterized functions.

**Architecture:** Centralize thunk materialization in `VirtualMachine::force_value`, refactor opcodes to use it, and add "auto-invocation" for top-level 0-arity functions in the manifestation layer.

**Tech Stack:** Rust, Bazel, Jsonnet.

---

### Task 1: Centralize Thunk Forcing in `force_value`

**Files:**
- Modify: `src/virtual_machine.rs`
- Test: `src/virtual_machine.rs` (unit tests)

- [ ] **Step 1: Update `force_value` implementation**

Modify `src/virtual_machine.rs` to handle `Value::Closure` thunks.

```rust
// In src/virtual_machine.rs
pub fn force_value(&mut self, val: Value) -> Result<Value, RuntimeError> {
    match val {
        Value::Import(_) => self.force_import(val),
        Value::Closure(ci) if self.memory_manager.load_closure(ci).is_thunk => {
            self.force_thunk(ci)
        }
        _ => Ok(val),
    }
}
```
*(Note: I'll need to move the import-forcing logic to a helper `force_import` or just inline it into the match.)*

- [ ] **Step 2: Add unit test for thunk forcing**

Add a test case to the `tests` module in `src/virtual_machine.rs`.

```rust
#[test]
fn test_force_value_thunk_materialization() {
    let mut chunk = create_test_chunk();
    // Create a thunk that returns 42
    let idx_42 = chunk.add_constant(Value::Number(42.0));
    chunk.write_opcode_u16(Opcode::LoadConst, idx_42 as u16, 0..5);
    chunk.write_opcode(Opcode::Return, 5..10);

    let mut mm = MemoryManager::new();
    let func_idx = mm.allocate_function(chunk.into_owned(), 0, 0, vec![]).index;
    let thunk_idx = mm.allocate_thunk(func_idx, vec![]).index;
    
    let mut vm = VirtualMachine::new(create_test_chunk(), mm);
    let result = vm.force_value(Value::Closure(thunk_idx)).unwrap();
    
    assert_eq!(result, Value::Number(42.0));
}
```

- [ ] **Step 3: Run unit tests**

Run: `bazel test //:virtual_machine_test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "feat: centralize thunk forcing in force_value"
```

---

### Task 2: Refactor Opcodes to use `force_value`

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Refactor `Opcode::LoadVar`**

Update the implementation of `Opcode::LoadVar` in the `interpret_until` loop to use `force_value`.

- [ ] **Step 2: Refactor `Opcode::GetUpvalue`**

Update the implementation of `Opcode::GetUpvalue` in the `interpret_until` loop to use `force_value`.

- [ ] **Step 3: Run existing tests to ensure no regressions**

Run: `bazel test //...`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "refactor: use force_value in LoadVar and GetUpvalue opcodes"
```

---

### Task 3: Ensure Native Functions receive forced data

**Files:**
- Modify: `src/virtual_machine.rs`
- Modify: `src/native.rs`

- [ ] **Step 1: Add element forcing to `std.join` in `src/native.rs`**

In `std_join`, ensure haystack elements are forced.

```rust
// In src/native.rs std_join
for elem in outer_elements.iter() {
    let forced_elem = vm.force_value(*elem)?; // Need to pass VM or force beforehand
    // ...
}
```
*(Wait, `native.rs` functions take `&mut MemoryManager`, not `&mut VirtualMachine`. I should force elements in `virtual_machine.rs` before calling the native function.)*

- [ ] **Step 2: Force array elements in `Opcode::StdCall`**

In `src/virtual_machine.rs`, before `call_native_checked`, if the function is `Join` or `Format`, force all array elements.

- [ ] **Step 3: Update `FormatVals::from_value` in `src/native.rs`**

Update `from_value` to take a closure or similar to force values if needed, or pre-force them in the VM.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs src/native.rs
git commit -m "feat: ensure native functions receive forced data"
```

---

### Task 4: Standalone Function Auto-Invocation and Manifestation Fix

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Implement auto-invocation in `execute_with_ext_vars`**

```rust
// In src/virtual_machine.rs execute_with_ext_vars
let mut value = vm.interpret()?;
if let Value::Closure(ci) = value {
    let func = vm.memory_manager.load_function(vm.memory_manager.load_closure(ci).function);
    if func.required_params == 0 {
        value = vm.call_test_closure(ci)?;
    }
}
```

- [ ] **Step 2: Update `value_to_json` to distinguish thunks from methods**

Only call `execute_thunk_sync_with_field` if `is_thunk` is true.

- [ ] **Step 3: Verify `realistic_2.jsonnet`**

Run: `bazel run //:main -- $(pwd)/benchmarks/extra/realistic_2.jsonnet`
Expected: SUCCESS with JSON output.

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "feat: support standalone execution of top-level 0-arity functions"
```

---

### Task 5: Regression Tests and Cleanup

**Files:**
- Create: `end2end/thunk_comprehension_join.jsonnet`
- Modify: `end2end/BUILD.bazel`

- [ ] **Step 1: Create regression test for `std.join` with thunks**

```jsonnet
local arr = [ i for i in [1, 2, 3] ];
std.assertEqual(std.join(",", [std.toString(x) for x in arr]), "1,2,3")
```

- [ ] **Step 2: Run all tests**

Run: `bazel test //...`
Expected: ALL PASS

- [ ] **Step 3: Cleanup temporary repro files**

Run: `rm repro.jsonnet repro_nested.jsonnet repro_thunk.jsonnet lazy_test.jsonnet`

- [ ] **Step 4: Final commit**

```bash
git add end2end/
git commit -m "test: add regression tests for thunk materialization"
```
