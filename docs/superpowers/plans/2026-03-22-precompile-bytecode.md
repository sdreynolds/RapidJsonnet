# Precompile Bytecode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add optional ahead-of-time bytecode compilation to the `jsonnet_library` Bazel rule via a `precompile_bytecode` attribute.

**Architecture:** Modify `jsonnet_aot` to accept an explicit output path, then update the `jsonnet_library` rule to optionally run the compiler and include the `.jsonnetc` output alongside the source in transitive depsets.

**Tech Stack:** Rust, Bazel (Starlark), shell

**Spec:** `docs/superpowers/specs/2026-03-22-precompile-bytecode-design.md`

---

### Task 1: Modify `jsonnet_aot` to accept an explicit output path

**Files:**
- Modify: `src/jsonnet_aot.rs`

The current compiler hardcodes the output path as `format!("{}c", filename)`. Bazel sandboxed actions require pre-declared output paths, so the compiler needs to accept an optional second argument for the output path.

- [ ] **Step 1: Write a test that verifies two-argument mode**

There is no existing test binary for `jsonnet_aot`. Instead, create a small end-to-end test file and test with a shell command.

Create `end2end/compiler_output_path.jsonnet`:
```jsonnet
{ value: 42 }
```

Run:
```bash
bazel build //:jsonnet_aot
bazel-bin/jsonnet_aot end2end/compiler_output_path.jsonnet /tmp/test_output.jsonnetc
```
Expected: should fail because two-arg mode is not yet implemented.

- [ ] **Step 2: Implement two-argument mode**

Modify `src/jsonnet_aot.rs` to handle both one-arg and two-arg invocation:

```rust
use compiler::Compiler;
use memory_manager::MemoryManager;
use scanner::Scanner;
use std::env;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    let filename = args
        .get(1)
        .expect("Usage: jsonnet_aot <input> [<output>]");

    let output_path = match args.get(2) {
        Some(path) => path.clone(),
        None => format!("{}c", filename),
    };

    let content = fs::read_to_string(filename)?;

    let mut scanner = Scanner::new(&content, filename);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, filename);
    let chunk = compiler
        .compile(&mut memory_manager)
        .map_err(|e| format!("Compilation failed: {:?}", e))?;

    let bytes = serialized_chunk::serialize_program(&chunk, &memory_manager);

    fs::write(&output_path, &bytes)?;

    println!(
        "Compiled {} -> {} ({} bytes)",
        filename,
        output_path,
        bytes.len()
    );

    Ok(())
}
```

- [ ] **Step 3: Verify both modes work**

```bash
# Two-arg mode (new):
bazel run //:jsonnet_aot -- $(pwd)/end2end/compiler_output_path.jsonnet /tmp/test_output.jsonnetc
ls -la /tmp/test_output.jsonnetc  # should exist

# One-arg mode (backwards compat):
bazel run //:jsonnet_aot -- $(pwd)/end2end/compiler_output_path.jsonnet
ls -la $(pwd)/end2end/compiler_output_path.jsonnetc  # should exist
```

Clean up test artifacts:
```bash
rm -f /tmp/test_output.jsonnetc end2end/compiler_output_path.jsonnetc end2end/compiler_output_path.jsonnet
```

- [ ] **Step 4: Commit**

```bash
git add src/jsonnet_aot.rs
git commit -m "feat: accept explicit output path in jsonnet_aot"
```

---

### Task 2: Add `precompile_bytecode` attribute to `jsonnet_library`

**Files:**
- Modify: `rules/jsonnet.bzl`

- [ ] **Step 1: Add the attribute and private `_compiler` tool to the rule definition**

In `rules/jsonnet.bzl`, add `precompile_bytecode` and `_compiler` to the `jsonnet_library` rule's `attrs` dict:

```python
        "precompile_bytecode": attr.bool(
            default = False,
            doc = "If True, ahead-of-time compile the source to bytecode using jsonnet_aot.",
        ),
        "_compiler": attr.label(
            default = Label("//:jsonnet_aot"),
            executable = True,
            cfg = "exec",
        ),
```

- [ ] **Step 2: Update `_jsonnet_library_impl` to conditionally compile**

Replace the `_jsonnet_library_impl` function with:

```python
def _jsonnet_library_impl(ctx):
    src_file = ctx.file.src
    transitive_srcs_deps, transitive_data_deps = _collect_transitive(ctx.attr.deps)

    direct_files = [src_file]

    if ctx.attr.precompile_bytecode:
        compiled = ctx.actions.declare_file(src_file.basename + "c")
        ctx.actions.run(
            outputs = [compiled],
            inputs = [src_file],
            executable = ctx.executable._compiler,
            arguments = [src_file.path, compiled.path],
            mnemonic = "JsonnetCompile",
            progress_message = "Precompiling Jsonnet bytecode: %s" % ctx.label,
        )
        direct_files.append(compiled)

    srcs_depset = depset([src_file])
    transitive_srcs_depset = depset(direct_files, transitive = transitive_srcs_deps)
    data_depset = depset(ctx.files.data, transitive = transitive_data_deps)

    all_files = depset(transitive = [transitive_srcs_depset, data_depset])

    return [
        DefaultInfo(files = all_files),
        JsonnetLibraryInfo(
            srcs = srcs_depset,
            transitive_srcs = transitive_srcs_depset,
            data = data_depset,
        ),
    ]
```

Key points:
- `srcs_depset` still contains only the raw source (used by `jsonnet_to_json` to find the entrypoint)
- `transitive_srcs_depset` includes both the source and the compiled bytecode, so both end up in downstream sandboxes
- When `precompile_bytecode = False`, behavior is identical to the current implementation

- [ ] **Step 3: Verify existing tests still pass**

```bash
bazel test //rules/tests/...
```
Expected: all existing tests pass (they don't use `precompile_bytecode`).

- [ ] **Step 4: Commit**

```bash
git add rules/jsonnet.bzl
git commit -m "feat: add precompile_bytecode attribute to jsonnet_library rule"
```

---

### Task 3: Add test for `precompile_bytecode = True`

**Files:**
- Modify: `rules/tests/BUILD.bazel`

- [ ] **Step 1: Add precompiled test targets to `rules/tests/BUILD.bazel`**

Add these targets after the existing ones:

```python
jsonnet_library(
    name = "utils_precompiled",
    src = "utils.libsonnet",
    precompile_bytecode = True,
)

jsonnet_library(
    name = "greeting_precompiled_lib",
    src = "greeting.jsonnet",
    deps = [":utils_precompiled"],
)

jsonnet_to_json(
    name = "greeting_precompiled_json",
    main = ":greeting_precompiled_lib",
)

sh_test(
    name = "precompiled_test",
    size = "small",
    srcs = ["verify_output.sh"],
    args = ["$(location :greeting_precompiled_json)"],
    data = [":greeting_precompiled_json"],
)
```

This test precompiles the `utils.libsonnet` dependency, then uses it via the normal `jsonnet_to_json` pipeline. The `verify_output.sh` script checks that the JSON output is correct — same as the non-precompiled tests.

- [ ] **Step 2: Run the new test**

```bash
bazel test //rules/tests:precompiled_test
```
Expected: PASS — the VM finds `utils.libsonnetc` in the sandbox and uses it instead of recompiling from source.

- [ ] **Step 3: Run all tests to verify nothing is broken**

```bash
bazel test //...
```
Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add rules/tests/BUILD.bazel
git commit -m "test: add precompile_bytecode integration test"
```
