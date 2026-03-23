# Precompile Bytecode for jsonnet_library

**Date:** 2026-03-22

## Summary

Add an optional `precompile_bytecode` boolean attribute to the `jsonnet_library` Bazel rule. When set to `True`, the rule runs `//:jsonnet_compiler` on the source file to produce a `.jsonnetc` bytecode artifact. Both the raw source and the precompiled bytecode are passed through in the transitive depsets, allowing the VM to pick up the cached bytecode at import time.

## Design

### Attribute

- **Name:** `precompile_bytecode`
- **Type:** `bool`
- **Default:** `False`
- **Location:** `jsonnet_library` rule in `rules/jsonnet.bzl`

### Prerequisite: `jsonnet_compiler` CLI change

The current `jsonnet_compiler` binary hardcodes output next to the input file (`format!("{}c", filename)`). This is incompatible with Bazel's sandboxed actions where inputs are read-only and outputs must be pre-declared. The compiler must be modified to accept an explicit output path: `jsonnet_compiler <input> <output>`. When only one argument is provided, the existing behavior (writing next to input) is preserved for backwards compatibility.

### Behavior when `precompile_bytecode = True`

1. The rule declares an output file using `ctx.actions.declare_file(src_file.basename + "c")` (e.g., `utils.libsonnet` → `utils.libsonnetc`). Using `basename` ensures the output is declared in the same package directory, so it ends up adjacent to the source file in the sandbox.
2. The rule runs `//:jsonnet_compiler` via `ctx.actions.run` with both the input source path and the declared output path as arguments.
3. Both the original source file and the precompiled bytecode file are included in `transitive_srcs` and propagated to downstream rules.
4. The bytecode file ends up next to the source file in the sandbox, so the VM's existing lookup logic (`{filename}c`) finds it automatically.

**Note:** Precompilation surfaces compile errors at `bazel build` time rather than at runtime. This only applies to `import` — `importstr` and `importbin` do not use bytecode lookup.

### Behavior when `precompile_bytecode = False` (default)

No change from current behavior. Only the raw source file is passed through.

### Private `_compiler` attribute

A new private `_compiler` attribute is added to `jsonnet_library`, pointing to `//:jsonnet_compiler` with `executable = True` and `cfg = "exec"`. Named `_compiler` (not `_tool`) to distinguish from `jsonnet_to_json`'s `_tool` which points to `//:main`.

### What stays the same

- `jsonnet_to_json` rule — unchanged. It already passes all transitive files into the sandbox.
- VM — unchanged. It already checks for `.jsonnetc` files and prefers them over raw source at import time.
- `JsonnetLibraryInfo` provider — unchanged. The `.jsonnetc` file is simply added to the existing depsets.

## Test plan

- Add a test in `rules/tests/` that uses `precompile_bytecode = True` on a `jsonnet_library` target and verifies the final JSON output is still correct via the existing `verify_output.sh` script.
