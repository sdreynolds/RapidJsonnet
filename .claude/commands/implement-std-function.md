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
- Do NOT require higher-order callback functions (no map, filter, foldl — those need separate VM infrastructure)
- Are unambiguous to implement from the spec

The PM writes a detailed implementation plan specifying: exact NativeFuncId assignments, arity, name strings, Rust implementation sketches, and required end2end test cases. If `$ARGUMENTS` were provided, the PM focuses on those specific functions instead.

### Developer
Reads all relevant source files before making any change. Implements exactly what the PM specified:
- Adds new variants to `NativeFuncId` enum in `src/chunk.rs` — must update ALL FOUR match blocks: enum discriminant, `from_u16`, `arity`, `name`, `from_name`
- Adds dispatch arms and implementation functions in `src/native.rs`
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
- GC safety if any VM preprocessing was added

If the Reviewer finds bugs, they report them precisely so the Developer can fix them. The loop continues until the Reviewer approves.

---

## Architecture Reminders for the Team

- `NativeFuncId` in `src/chunk.rs`: add to enum + `from_u16` + `arity` + `name` + `from_name` (4 places)
- Current max NativeFuncId: `ParseFloat = 73` — new functions start at 74
- String values: `Value::String(StringIndex)` — use `mm.lookup_string(idx)` to get `&str`
- Array allocation: `mm.allocate_array(vec)` returns an `ArrayAlloc` with `.index`
- String interning: `mm.intern_string(s)` returns a `StringIndex`
- Two-phase borrow: clone object/array data out before re-borrowing `memory_manager` mutably
- VM preprocessing pattern (for functions needing callbacks): see `Sort`, `Uniq`, `Format`, `Get` in `virtual_machine.rs`
- Rustfmt: long `format!()` calls must use multi-line style

---

## Execution

Spawn the three agents sequentially:

1. **Spawn PM agent** (Plan subagent): Read `docs/jsonnet_std.md` and `src/chunk.rs`. Identify unimplemented functions. Pick 5–8 best candidates. Produce a detailed implementation plan.

2. **Spawn Developer agent** (general-purpose, bypassPermissions): Read all relevant source files first. Implement the PM's plan exactly. Build, test, and fix until all tests pass and rustfmt is clean.

3. **Spawn Reviewer agent** (Explore subagent): Run `bazel test //...` and `bazel build --config=rustfmt //...`. Read the implementation. Report spec compliance, bugs, and edge case coverage. Approve or request changes.

After Reviewer approves, report the summary of what was implemented and wait for the user to commit.
