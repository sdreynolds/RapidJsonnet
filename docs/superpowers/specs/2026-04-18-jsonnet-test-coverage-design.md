# Design: `jsonnet_test` Bazel Coverage Integration

**Date:** 2026-04-18  
**Status:** Approved

## Background

RapidJsonnet uses a custom `jsonnet_test` Bazel rule to run `.jsonnet` test files through the interpreter. The rule has a `coverage = "lcov"` attribute that writes Jsonnet source coverage (which spans in `.jsonnet` library files were executed) to `$TEST_UNDECLARED_OUTPUTS_DIR`. However, `jsonnet_test` targets are absent from `bazel coverage --combined_report=lcov` output because the rule does not declare `InstrumentedFilesInfo` and does not write to `$COVERAGE_OUTPUT_FILE`.

The goal is to wire `jsonnet_test` into Bazel's standard coverage protocol so that Jsonnet source coverage is included in the combined LCOV report.

## What Coverage Means Here

Coverage is **Jsonnet source coverage**: which lines/spans in `.jsonnet` library files are exercised when the test runs. This is distinct from Rust interpreter coverage (which Rust code paths are exercised). The `jsonnet_test_runner` binary already collects this data via `CoverageCollector` and generates LCOV output; the missing piece is wiring it into Bazel's protocol.

## Changes

### `rules/jsonnet.bzl`

Add `coverage_common.instrumented_files_info()` to the return value of `_jsonnet_test_impl`:

```python
return [
    DefaultInfo(executable = wrapper, runfiles = runfiles),
    coverage_common.instrumented_files_info(
        ctx,
        source_attributes = ["src"],
        dependency_attributes = ["deps"],
    ),
]
```

`source_attributes = ["src"]` registers the direct `.jsonnet` source file. `dependency_attributes = ["deps"]` lets Bazel traverse transitive `jsonnet_library` deps automatically.

No other changes to the rule. The `coverage` attribute is retained on the rule definition for the `TEST_UNDECLARED_OUTPUTS_DIR` use case but is no longer used at any call site.

### `src/jsonnet_test_runner.rs`

Two additions:

**1. Auto-enable coverage when Bazel coverage mode is active.** After arg parsing, before running tests:

```rust
if std::env::var("COVERAGE_OUTPUT_FILE").is_ok() {
    collect_coverage = true;
}
```

This means `--coverage` is not needed on the command line during `bazel coverage` runs.

**2. Write LCOV to `$COVERAGE_OUTPUT_FILE`.** In the existing coverage output block, alongside the `COVERAGE_DIR` and `TEST_UNDECLARED_OUTPUTS_DIR` checks:

```rust
if let Ok(cov_file) = std::env::var("COVERAGE_OUTPUT_FILE") {
    if let Err(e) = fs::write(&cov_file, &lcov_content) {
        eprintln!("Warning: failed to write coverage to {}: {}", cov_file, e);
    }
}
```

Bazel's `lcov_merger` reads this file to build the combined report.

### `end2end/BUILD.bazel`

Remove `coverage = "lcov"` from all three `jsonnet_test` targets:
- `test_framework_integration_test`
- `test_filter_framework_integration_test`
- `skip_test_framework_integration_test`

Under `bazel coverage`, `$COVERAGE_OUTPUT_FILE` triggers coverage automatically via the runner change above, making the attribute redundant.

## Behavior After Change

| Command | Coverage collected? | Output |
|---|---|---|
| `bazel test //end2end:*` | No | Normal test pass/fail |
| `bazel coverage //end2end:*` | Yes | LCOV written to `$COVERAGE_OUTPUT_FILE`, picked up by `lcov_merger` |
| Runner with `--coverage` flag | Yes | LCOV written to `--lcov-output` path and/or `$TEST_UNDECLARED_OUTPUTS_DIR` |

## Testing

- `bazel test //end2end:test_framework_integration_test` — confirms normal test run still works with `coverage = "lcov"` removed
- `bazel coverage //end2end:test_framework_integration_test --combined_report=lcov` — confirms the target appears in `bazel-out/_coverage/_coverage_report.dat` with `.jsonnet` source paths in the LCOV entries
