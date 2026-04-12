# Bazel Jsonnet Rules Design Spec

## Goal

Create two Bazel rules (`jsonnet_library` and `jsonnet_to_json`) that allow users to build JSON files from Jsonnet source using the RapidJsonnet binary.

## Rules

### Provider: `JsonnetLibraryInfo`

A custom Starlark provider carrying:
- `srcs` — depset of the library's own source file
- `transitive_srcs` — depset of all source files from the library and its transitive deps
- `data` — depset of data files from the library and its transitive deps

### `jsonnet_library`

Copies Jsonnet source files into the sandbox so they are available to downstream rules.

**Attributes:**
- `src` (label, mandatory) — single `.jsonnet` or `.libsonnet` file
- `deps` (list of labels, optional) — must be `jsonnet_library` targets
- `data` (list of labels, optional) — arbitrary files/targets

**Behavior:**
- Collects transitive sources and data from deps via `JsonnetLibraryInfo`
- Returns `JsonnetLibraryInfo` with merged depsets
- Returns `DefaultInfo` with all files (own + transitive)

### `jsonnet_to_json`

Executes the RapidJsonnet binary to produce a JSON output file.

**Attributes:**
- `main` (label, mandatory) — a `jsonnet_library` target providing the entrypoint
- `deps` (list of labels, optional) — additional `jsonnet_library` targets
- `data` (list of labels, optional) — arbitrary files/targets
- `out` (string, optional) — output filename; defaults to `<target_name>.json`

**Behavior:**
- Collects all transitive sources and data from `main` + `deps`
- Declares output file named per `out` attribute (or `<name>.json`)
- Runs `//:main -q <main_src>` redirecting stdout to the output file
- All collected files are sandbox inputs so relative imports resolve
- Returns `DefaultInfo` with the JSON output file

## File Structure

```
rules/
  jsonnet.bzl    — both rules + JsonnetLibraryInfo provider
  BUILD.bazel    — empty package file
```

## Out of Scope

- `ext_str` / `ext_code` passthrough
- JPATH / `--jpath` support
- Multi-file output
- Validation of `.jsonnet` / `.libsonnet` extensions on `src`
