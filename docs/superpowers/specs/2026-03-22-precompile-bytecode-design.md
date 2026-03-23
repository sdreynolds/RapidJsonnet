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

### Behavior when `precompile_bytecode = True`

1. The rule declares an output file with the same path as the source file plus a `c` suffix (e.g., `utils.libsonnet` → `utils.libsonnetc`).
2. The rule runs `//:jsonnet_compiler` via `ctx.actions.run` to compile the source into the `.jsonnetc` output.
3. Both the original source file and the `.jsonnetc` file are included in `transitive_srcs` and propagated to downstream rules.
4. The `.jsonnetc` file is placed next to the source file in the sandbox so the VM's existing lookup logic (`{filename}c`) finds it.

### Behavior when `precompile_bytecode = False` (default)

No change from current behavior. Only the raw source file is passed through.

### Private `_tool` attribute

A new private `_tool` attribute is added to `jsonnet_library`, pointing to `//:jsonnet_compiler` with `executable = True` and `cfg = "exec"`. This mirrors the pattern used by `jsonnet_to_json` for `//:main`.

### What stays the same

- `jsonnet_to_json` rule — unchanged. It already passes all transitive files into the sandbox.
- VM — unchanged. It already checks for `.jsonnetc` files and prefers them over raw source at import time.
- `JsonnetLibraryInfo` provider — unchanged. The `.jsonnetc` file is simply added to the existing depsets.

## Test plan

- Add a test in `rules/tests/` that uses `precompile_bytecode = True` on a `jsonnet_library` target and verifies the final JSON output is still correct via the existing `verify_output.sh` script.
