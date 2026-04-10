# Design Spec: Thunk Materialization and Standalone Function Execution

This document outlines the architectural changes required to fix the "thunk is not materialized" error in RapidJsonnet and to support the standalone execution of top-level parameterized functions (required for benchmarking `realistic_2.jsonnet`).

## Problem Statement

RapidJsonnet's current thunk-forcing logic is fragmented and incomplete:
1.  `force_value` in `virtual_machine.rs` only handles `Value::Import`, ignoring `Value::Closure` even when marked as `is_thunk`.
2.  Native functions (like `std.join` and `std.format`) often receive arrays/objects containing unforced thunks (from comprehensions), leading to crashes.
3.  The manifestation logic (`value_to_json`) incorrectly attempts to execute regular methods (arity 2) as thunks, and fails to handle top-level functions that require 0 arguments (like `realistic_2.jsonnet`).

## Proposed Architecture

### 1. Centralized Thunk Materialization (`src/virtual_machine.rs`)

The `force_value` method will become the single source of truth for "materializing" any lazy value into a concrete Jsonnet value.

- **`force_value(Value)`**:
    - If `Value::Import`: Existing logic (load, compile, evaluate, cache).
    - If `Value::Closure` and `is_thunk == true`: Call `force_thunk(closure_idx)` and return result.
    - Otherwise: Return the value as-is.

- **Refactor Opcodes**:
    - `Opcode::LoadVar` and `Opcode::GetUpvalue` will be simplified to call `force_value` after retrieving the value from the stack/heap, rather than implementing their own thunk-forcing logic.

### 2. Native Function Integrity (`src/native.rs` and `src/virtual_machine.rs`)

Native functions expect concrete data, not lazy computations. We must ensure "container" values (Arrays and Objects) are fully forced before they enter "Rust land."

- **`Opcode::StdCall` (VM)**: Before invoking `call_native_checked`, for any argument that is a `Value::Array`, call `self.force_all_array_elements(array_key)`.
- **`std.join` (Native)**: In the array separator path, ensure each element of the haystack is forced via `self.force_value(elem)` before matching against `Value::Array`.
- **`std.format` (Native)**: Update `FormatVals::from_value` to call `self.force_value(v)` on every element/field before storing it in the `FormatVals` enum.

### 3. Standalone Function Execution (`src/virtual_machine.rs`)

To support benchmarking files that return a top-level function literal (like `realistic_2.jsonnet`), we will implement an "auto-invocation" step at the CLI/TLA layer.

- **`execute_with_ext_vars`**:
    - After `vm.interpret()` returns the result `V`.
    - If `V` is a `Value::Closure` and its `ManagedFunction` has `required_params == 0`:
        - Invoke it with 0 arguments using `vm.call_test_closure(closure_idx)`.
        - The resulting value becomes the final value for manifestation.
    - If `V` is a `Value::Closure` and `required_params > 0`:
        - Do NOT invoke. Proceed to manifestation, which will trigger the standard Jsonnet error: "Attempting to manifest a function raises an error since they do not exist in JSON" (as per `docs/jsonnet_spec.md`).

- **Manifestation Refinement**:
    - Update `value_to_json` to only call `execute_thunk_sync_with_field` if the closure is explicitly marked as `is_thunk`. 
    - Regular methods and functions will now correctly trigger a serialization error (`RuntimeError`) instead of a VM crash or incorrect execution. This aligns with the operational semantics in the spec where `Function ⇓ j` is stuck execution.

## Testing Strategy

### Unit Tests
- `test_force_value_thunk`: Verify `force_value` correctly materializes a thunk closure.
- `test_standalone_function_execution`: Verify that a top-level function with 0 required parameters is automatically invoked.

### Integration Tests
- `end2end/thunk_comprehension_join.jsonnet`: A test case that passes a comprehension result (array of thunks) to `std.join`.
- `benchmarks/extra/realistic_2.jsonnet`: Verify this file now runs to completion and produces valid JSON.

## Success Criteria
- `realistic_2.jsonnet` executes and manifests without error.
- No regressions in existing `end2end` tests or `test_suite`.
- Thunks are consistently forced before they reach native functions or manifestation.
