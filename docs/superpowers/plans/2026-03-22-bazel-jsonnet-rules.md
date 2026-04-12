# Bazel Jsonnet Rules Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create `jsonnet_library` and `jsonnet_to_json` Bazel rules that compile Jsonnet to JSON using the RapidJsonnet binary.

**Architecture:** A single `.bzl` file in `rules/` defines a `JsonnetLibraryInfo` provider for transitive dependency tracking, a `jsonnet_library` rule that collects source and data files into depsets, and a `jsonnet_to_json` rule that runs `//:main -q` to produce JSON output.

**Tech Stack:** Starlark (Bazel rule language), Bazel 8.3.1 with Bzlmod

---

### Task 1: Create the rules package

**Files:**
- Create: `rules/BUILD.bazel`
- Create: `rules/jsonnet.bzl`

- [ ] **Step 1: Create empty BUILD.bazel for the rules package**

```python
# rules/BUILD.bazel
# Empty — makes this directory a Bazel package
```

- [ ] **Step 2: Create `rules/jsonnet.bzl` with the provider and both rules**

```python
"""Bazel rules for building JSON from Jsonnet using RapidJsonnet."""

JsonnetLibraryInfo = provider(
    doc = "Provides transitive Jsonnet sources and data files.",
    fields = {
        "srcs": "depset of this library's source file",
        "transitive_srcs": "depset of all transitive Jsonnet source files",
        "data": "depset of all transitive data files",
    },
)

def _collect_transitive(deps):
    """Collect transitive srcs and data from JsonnetLibraryInfo deps."""
    transitive_srcs = []
    transitive_data = []
    for dep in deps:
        if JsonnetLibraryInfo in dep:
            info = dep[JsonnetLibraryInfo]
            transitive_srcs.append(info.transitive_srcs)
            transitive_data.append(info.data)
    return transitive_srcs, transitive_data

def _jsonnet_library_impl(ctx):
    src_file = ctx.file.src
    transitive_srcs_deps, transitive_data_deps = _collect_transitive(ctx.attr.deps)

    srcs_depset = depset([src_file])
    transitive_srcs_depset = depset([src_file], transitive = transitive_srcs_deps)
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

jsonnet_library = rule(
    implementation = _jsonnet_library_impl,
    attrs = {
        "src": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "A single Jsonnet source file.",
        ),
        "deps": attr.label_list(
            providers = [JsonnetLibraryInfo],
            doc = "jsonnet_library dependencies.",
        ),
        "data": attr.label_list(
            allow_files = True,
            doc = "Data files available at runtime.",
        ),
    },
    doc = "Collects a Jsonnet source file and its dependencies for use by jsonnet_to_json.",
)

def _jsonnet_to_json_impl(ctx):
    # Resolve output filename
    out_name = ctx.attr.out if ctx.attr.out else ctx.label.name + ".json"
    output = ctx.actions.declare_file(out_name)

    # Get the main source file
    main_info = ctx.attr.main[JsonnetLibraryInfo]
    main_src = main_info.srcs.to_list()[0]

    # Collect all transitive inputs from main + deps
    transitive_srcs_deps = [main_info.transitive_srcs]
    transitive_data_deps = [main_info.data]

    dep_srcs, dep_data = _collect_transitive(ctx.attr.deps)
    transitive_srcs_deps.extend(dep_srcs)
    transitive_data_deps.extend(dep_data)

    all_srcs = depset(transitive = transitive_srcs_deps)
    all_data = depset(ctx.files.data, transitive = transitive_data_deps)
    all_inputs = depset(transitive = [all_srcs, all_data])

    ctx.actions.run_shell(
        outputs = [output],
        inputs = all_inputs,
        tools = [ctx.executable._tool],
        command = "{tool} -q {src} > {out}".format(
            tool = ctx.executable._tool.path,
            src = main_src.path,
            out = output.path,
        ),
        mnemonic = "Jsonnet",
        progress_message = "Compiling Jsonnet to JSON: %s" % ctx.label,
    )

    return [DefaultInfo(files = depset([output]))]

jsonnet_to_json = rule(
    implementation = _jsonnet_to_json_impl,
    attrs = {
        "main": attr.label(
            providers = [JsonnetLibraryInfo],
            mandatory = True,
            doc = "The jsonnet_library target containing the entrypoint file.",
        ),
        "deps": attr.label_list(
            providers = [JsonnetLibraryInfo],
            doc = "Additional jsonnet_library dependencies.",
        ),
        "data": attr.label_list(
            allow_files = True,
            doc = "Data files available at runtime.",
        ),
        "out": attr.string(
            doc = "Output filename. Defaults to <target_name>.json.",
        ),
        "_tool": attr.label(
            default = Label("//:main"),
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Compiles a Jsonnet file to JSON using the RapidJsonnet binary.",
)
```

- [ ] **Step 3: Commit**

```bash
git add rules/BUILD.bazel rules/jsonnet.bzl
git commit -m "feat: add jsonnet_library and jsonnet_to_json Bazel rules"
```

---

### Task 2: Create an end-to-end test

**Files:**
- Create: `rules/tests/BUILD.bazel`
- Create: `rules/tests/greeting.jsonnet`
- Create: `rules/tests/utils.libsonnet`
- Create: `rules/tests/verify_output.sh`

- [ ] **Step 1: Create test Jsonnet files**

`rules/tests/utils.libsonnet`:
```jsonnet
{
  greet(name):: "Hello, " + name + "!",
}
```

`rules/tests/greeting.jsonnet`:
```jsonnet
local utils = import "utils.libsonnet";

{
  message: utils.greet("World"),
}
```

- [ ] **Step 2: Create the test BUILD file**

`rules/tests/BUILD.bazel`:
```python
load("//rules:jsonnet.bzl", "jsonnet_library", "jsonnet_to_json")

jsonnet_library(
    name = "utils",
    src = "utils.libsonnet",
)

jsonnet_library(
    name = "greeting_lib",
    src = "greeting.jsonnet",
    deps = [":utils"],
)

jsonnet_to_json(
    name = "greeting_json",
    main = ":greeting_lib",
)

jsonnet_to_json(
    name = "greeting_custom_out",
    main = ":greeting_lib",
    out = "custom_name.json",
)

sh_test(
    name = "greeting_test",
    srcs = ["verify_output.sh"],
    args = ["$(location :greeting_json)"],
    data = [":greeting_json"],
)

sh_test(
    name = "custom_out_test",
    srcs = ["verify_output.sh"],
    args = ["$(location :greeting_custom_out)"],
    data = [":greeting_custom_out"],
)
```

- [ ] **Step 3: Create the verification script**

`rules/tests/verify_output.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

OUTPUT_FILE="$1"

# Check file exists
if [ ! -f "$OUTPUT_FILE" ]; then
  echo "FAIL: Output file not found: $OUTPUT_FILE"
  exit 1
fi

# Check content contains expected greeting
if grep -q '"Hello, World!"' "$OUTPUT_FILE"; then
  echo "PASS: Output contains expected greeting"
else
  echo "FAIL: Expected '\"Hello, World!\"' in output, got:"
  cat "$OUTPUT_FILE"
  exit 1
fi
```

- [ ] **Step 4: Run the tests**

```bash
bazel test //rules/tests:greeting_test //rules/tests:custom_out_test
```

Expected: Both tests PASS.

- [ ] **Step 5: Commit**

```bash
git add rules/tests/
git commit -m "test: add end-to-end tests for jsonnet Bazel rules"
```
