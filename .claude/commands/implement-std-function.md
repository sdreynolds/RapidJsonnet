---
description: Run the 3-agent team (Project Manager → Developer → Reviewer) to implement the next batch of Jsonnet std library functions in RapidJsonnet.
argument-hint: [optional: specific function names to implement]
---

Run the RapidJsonnet std library implementation team. The team consists of three agents with distinct roles:

---

## Agent Roles

### Project Manager
Reads `docs/jsonnet_std.md` and `src/chunk.rs` to determine what has and hasn't been implemented yet. Picks the next most fundamental batch of 5–8 functions that:
- Have real daily utility in Jsonnet's primary use cases (config generation, string templating, data transformation)
- Are unambiguous to implement from the spec

Higher-order callback functions (map, filter, foldl, etc.) ARE now supported — the VM has `call_value_with_one_arg` and `call_value_with_two_args` helpers and the GC-safe rooting pattern. Include them when they are the most valuable remaining functions.

The PM writes a detailed implementation plan specifying: exact NativeFuncId assignments, arity, name strings, Rust implementation sketches, and required end2end test cases. If `$ARGUMENTS` were provided, the PM focuses on those specific functions instead.

### Developer
Reads all relevant source files before making any change. Implements exactly what the PM specified:
- Adds new variants to `NativeFuncId` enum in `src/chunk.rs` — must update ALL FIVE match blocks: enum discriminant, `from_u16`, `arity`, `name`, `from_name`
- Adds dispatch arms and implementation functions in `src/native.rs`
- For VM-special functions (callbacks, object thunks, JSON parsing): adds preprocessing blocks in `src/virtual_machine.rs` before the `call_native` fallthrough
- Creates end2end test files in `end2end/` (auto-discovered by `glob(["*.jsonnet"])`, no BUILD changes needed)
- Error test files must contain "error" in the filename (they expect non-zero exit)
- Runs `bazel build //...` and `bazel test //...` — fixes any failures
- Runs `bazel build --config=rustfmt //...` and fixes format issues with `bazel run @rules_rust//:rustfmt`
- NEVER trusts LSP/rust-analyzer warnings — those are always false positives in this Bazel project

### Reviewer
Runs the full test suite and format check, reads the implementation, and verifies:
- All tests pass (`bazel test //...`)
- Rustfmt passes (`bazel build --config=rustfmt //...`)
- Each function matches the Jsonnet spec in `docs/jsonnet_std.md`
- Edge cases are covered (empty inputs, type errors, boundary values)
- No regressions in existing tests
- GC safety for any HOF or VM preprocessing loops
- Object field closures are force-evaluated where needed (objectValues, manifestJson, etc.)

If the Reviewer finds bugs, they report them precisely so the Developer can fix them. The loop continues until the Reviewer approves.

---

## Architecture Reminders for the Team

### chunk.rs
- `NativeFuncId` in `src/chunk.rs`: add to enum + `from_u16` + `arity` + `name` + `from_name` (4 places)
- **Current max NativeFuncId: `Product = 163`** — new functions start at 164
- Aliases (e.g. `escapeStringPython`): add only to `from_name`, mapping to an existing variant — no new enum variant needed

### native.rs
- String values: `Value::String(StringIndex)` — use `mm.load_string(idx)` to get `&str`
- Array allocation: `mm.allocate_array(vec)` returns an `ArrayAlloc` with `.index`
- String interning: `mm.intern_string(s)` returns a `StringIndex`
- Two-phase borrow: clone object/array data out before re-borrowing `memory_manager` mutably
- `values_equal`, `compare_values`, `value_sort_key` are all `pub` — use them directly
- VM-special stubs: functions that need VM callbacks get an error stub in `call_native`:
  ```rust
  NativeFuncId::Foo => Err(RuntimeError {
      span, message: format!("std.{} must be handled by the VM", id.name()), source_id,
  }),
  ```

### virtual_machine.rs — VM preprocessing patterns

**CRITICAL: VM preprocessing handlers must be added at TWO dispatch sites** in `virtual_machine.rs`. Search for existing HOF handlers (e.g., `NativeFuncId::GroupBy`) to find both locations (~line 3576 and ~line 4934). Both sites must have identical handler code.

**Single-arg callback (map, filter, sort keyF, uniq keyF):**
Use `call_value_with_one_arg(func, elem)` helper. Before each call, GC-root accumulated results:
```rust
let mut roots = Vec::from(self.stack.clone());
roots.extend_from_slice(&elements);
roots.extend_from_slice(&results);
roots.push(func_val);
// collect open upvalues...
self.memory_manager.push_external_roots(roots, open_upvalue_roots);
let result = self.call_value_with_one_arg(func_val, elem)?;
self.memory_manager.pop_external_roots();
```

**Two-arg callback (foldl, mapWithIndex):**
Use `call_value_with_two_args(func, arg1, arg2)` helper. Same GC rooting pattern; also root the accumulator value.

**Sort-by-key pattern (sortBy, minBy, maxBy):**
Do NOT call keyF inside a sort comparator — `sort_by` holds a mutable closure that conflicts with re-borrowing `memory_manager`. Instead, pre-compute all keys first: build `Vec<(key_value, orig_value)>`, then sort on pre-computed keys using `compare_values`, then extract original values.

**Object field thunk evaluation (objectValues, objectValuesAll, objectKeysValues, objectKeysValuesAll, manifestJson):**
Force closures before passing to `call_native` using `execute_thunk_sync(closure_idx, Some(o_idx), super_obj)`. This preprocessing block runs for all four object introspection functions.

**Compiler constants (std.pi):**
Add `else if name == "pi"` branch near `thisFile` in `compiler.rs` to emit `LoadConst` of `Value::Number(std::f64::consts::PI)` at compile time — no NativeFuncId needed.

**parseJson:** Uses `serde_json` (already a dep of `virtual_machine`). Convert via `json_to_jsonnet_value` helper method on VirtualMachine.

**manifestJson/manifestJsonEx/manifestJsonMinified:** VM-special; implemented as `manifest_json_value` recursive method. Uses `execute_thunk_sync` for object field closures. Empty array → `"[ ]"`, empty object → `"{ }"`. Default: 3-space indent, `\n` newline, `": "` key-val sep.

### Rustfmt
- Long `format!()` calls must use multi-line style
- Run `bazel run @rules_rust//:rustfmt` to auto-fix

---

## Currently Remaining Gaps (as of NativeFuncId=163)

Sprints 1–7 implemented IDs 120–163, covering:
- HOFs: map, filter, foldl, mapWithIndex, sortBy, countBy, uniqBy, minBy, maxBy
- Set ops: setUnion, setInter, setDiff, setMember (with optional keyF)
- Encoding: md5, sha1, sha256, sha512, sha3 (via external crates in MODULE.bazel)
- Object utils: objectValues, objectValuesAll, objectKeysValues, objectKeysValuesAll, objectFromPairs, objectFlatten, pick, omit, toPairs, mapKeys, filterObject, groupBy
- String: indent
- Math: gcd, lcm
- Parse: parseJson, parseYaml (via serde_yaml crate)
- Manifest: manifestJson, manifestJsonEx, manifestJsonMinified, manifestYamlDoc, manifestYamlStream
- Array: chunk, zip, unzip, product
- Misc: extVar (via CLI --ext-str KEY=VALUE), mapWithKey, filterMap

**Still unimplemented:**
- **HOFs**: `foldr` (right fold), `filterMap` (if not yet done)
- **Math**: `mantissa(x)`, `exponent(x)` — IEEE frexp analogs; low utility
- **Manifest formats**: `manifestIni`, `manifestTomlEx`, `manifestPython`, `manifestPythonVars`, `manifestXmlJsonml` — complex serializers
- **Misc**: any remaining aliases not yet wired in `from_name`

> **Note**: Before the PM picks functions, always re-read `src/chunk.rs` `from_name` to get the definitive list of what's actually registered. The list above may be slightly stale.

---

## Execution

Spawn the three agents sequentially:

1. **Spawn PM agent** (`Plan` subagent): Read `docs/jsonnet_std.md` and `src/chunk.rs`. Identify unimplemented functions. Pick 5–8 best candidates. Produce a detailed implementation plan.

2. **Spawn Developer agent** (general-purpose, `bypassPermissions`): Read all relevant source files first. Implement the PM's plan exactly. Build, test, and fix until all tests pass and rustfmt is clean.

3. **Spawn Reviewer agent** (`Explore` subagent): Run `bazel test //...` and `bazel build --config=rustfmt //...`. Read the implementation. Report spec compliance, bugs, and edge case coverage. Approve or request changes.

After Reviewer approves, report the summary of what was implemented and wait for the user to commit.
