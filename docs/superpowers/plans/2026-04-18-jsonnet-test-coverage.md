# jsonnet_test Coverage Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `jsonnet_test` into `bazel coverage --combined_report=lcov` so Jsonnet source coverage is included in the combined LCOV report.

**Architecture:** Three targeted edits across three files. The Bazel rule declares `InstrumentedFilesInfo` so Bazel knows which `.jsonnet` files to track. The runner binary auto-enables coverage and writes to `$COVERAGE_OUTPUT_FILE` when Bazel sets it. Call sites drop the now-redundant `coverage = "lcov"` attribute.

**Tech Stack:** Starlark (Bazel rules), Rust (`jsonnet_test_runner` binary), Bazel `coverage_common` API

---

## File Map

| File | Change |
|---|---|
| `rules/jsonnet.bzl` | Add `coverage_common.instrumented_files_info()` to `_jsonnet_test_impl` return |
| `src/jsonnet_test_runner.rs` | Auto-enable coverage + write to `$COVERAGE_OUTPUT_FILE` |
| `end2end/BUILD.bazel` | Remove `coverage = "lcov"` from 3 `jsonnet_test` targets |

---

### Task 1: Add `InstrumentedFilesInfo` to `_jsonnet_test_impl`

**Files:**
- Modify: `rules/jsonnet.bzl:191-196`

`InstrumentedFilesInfo` is the Bazel provider that tells `bazel coverage` which source files a test covers. Without it, Bazel skips the target entirely during coverage collection. `coverage_common.instrumented_files_info()` is the standard API — `source_attributes` names the rule attribute holding direct source files; `dependency_attributes` names dep attributes so Bazel traverses transitive libraries automatically.

- [ ] **Step 1: Edit `_jsonnet_test_impl` return**

In `rules/jsonnet.bzl`, replace the final `return` statement in `_jsonnet_test_impl` (currently at lines 191–196):

```python
    return [
        DefaultInfo(
            executable = wrapper,
            runfiles = runfiles,
        ),
    ]
```

with:

```python
    return [
        DefaultInfo(
            executable = wrapper,
            runfiles = runfiles,
        ),
        coverage_common.instrumented_files_info(
            ctx,
            source_attributes = ["src"],
            dependency_attributes = ["deps"],
        ),
    ]
```

- [ ] **Step 2: Verify the rule still builds**

```bash
bazel build //end2end:test_framework_integration_test
```

Expected: Build succeeds with no errors.

- [ ] **Step 3: Commit**

```bash
git add rules/jsonnet.bzl
git commit -m "feat: add InstrumentedFilesInfo to jsonnet_test rule"
```

---

### Task 2: Update `jsonnet_test_runner` to handle `$COVERAGE_OUTPUT_FILE`

**Files:**
- Modify: `src/jsonnet_test_runner.rs:77-78` (after arg-parsing loop)
- Modify: `src/jsonnet_test_runner.rs:163-170` (coverage output block)

When `bazel coverage` runs a test, it sets `$COVERAGE_OUTPUT_FILE` to the path where the test must write its LCOV data. The `lcov_merger` tool then reads all such files to build the combined report. The runner already writes to `$COVERAGE_DIR/coverage.dat` and `$TEST_UNDECLARED_OUTPUTS_DIR`; this task adds the `$COVERAGE_OUTPUT_FILE` path.

Coverage collection also needs to be auto-enabled when `$COVERAGE_OUTPUT_FILE` is set, because the `--coverage` CLI flag won't be present during `bazel coverage` runs (the wrapper doesn't pass it).

- [ ] **Step 1: Auto-enable `collect_coverage` when `COVERAGE_OUTPUT_FILE` is set**

In `src/jsonnet_test_runner.rs`, after the arg-parsing `while` loop ends (after line 77, before `let filename = ...` on line 79), add:

```rust
    // Auto-enable coverage when Bazel coverage mode is active.
    if std::env::var("COVERAGE_OUTPUT_FILE").is_ok() {
        collect_coverage = true;
    }
```

- [ ] **Step 2: Write LCOV to `$COVERAGE_OUTPUT_FILE`**

In `src/jsonnet_test_runner.rs`, inside the `if needs_lcov {` block, after the existing `COVERAGE_DIR` check (after line 170), add:

```rust
                    // bazel coverage: write LCOV to the file Bazel's lcov_merger expects.
                    if let Ok(cov_file) = std::env::var("COVERAGE_OUTPUT_FILE") {
                        if let Err(e) = fs::write(&cov_file, &lcov_content) {
                            eprintln!(
                                "Warning: failed to write coverage to {}: {}",
                                cov_file, e
                            );
                        }
                    }
```

The full updated coverage output block should read:

```rust
                if needs_lcov {
                    let tn = suite_name.as_deref().unwrap_or("");
                    let lcov_content = generate_lcov(coverage, tn);
                    // Explicit --lcov-output path
                    if let Some(path) = &lcov_output {
                        if let Err(e) = fs::write(path, &lcov_content) {
                            eprintln!("Warning: failed to write LCOV to {}: {}", path, e);
                        }
                    }
                    // bazel test: write to undeclared outputs so the file appears at
                    // bazel-testlogs/<pkg>/<target>/test.outputs/<test_name>.lcov
                    if let Ok(undeclared_dir) = std::env::var("TEST_UNDECLARED_OUTPUTS_DIR") {
                        let lcov_filename = if tn.is_empty() {
                            "coverage.lcov".to_string()
                        } else {
                            format!("{}.lcov", tn)
                        };
                        let out = std::path::Path::new(&undeclared_dir).join(&lcov_filename);
                        if let Err(e) = fs::write(&out, &lcov_content) {
                            eprintln!("Warning: failed to write {}: {}", lcov_filename, e);
                        }
                    }
                    // bazel coverage: write coverage.dat for COVERAGE_DIR consumers.
                    if let Ok(coverage_dir) = std::env::var("COVERAGE_DIR") {
                        let dat = std::path::Path::new(&coverage_dir).join("coverage.dat");
                        if let Err(e) = fs::write(&dat, &lcov_content) {
                            eprintln!("Warning: failed to write coverage.dat: {}", e);
                        }
                    }
                    // bazel coverage: write LCOV to the file Bazel's lcov_merger expects.
                    if let Ok(cov_file) = std::env::var("COVERAGE_OUTPUT_FILE") {
                        if let Err(e) = fs::write(&cov_file, &lcov_content) {
                            eprintln!(
                                "Warning: failed to write coverage to {}: {}",
                                cov_file, e
                            );
                        }
                    }
                }
```

- [ ] **Step 3: Verify the binary still builds**

```bash
bazel build //:jsonnet_test_runner
```

Expected: Build succeeds with no errors.

- [ ] **Step 4: Verify normal test run is unaffected**

```bash
bazel test //end2end:test_framework_integration_test
```

Expected: PASSED — no coverage output triggered (no `$COVERAGE_OUTPUT_FILE` set during plain `bazel test`).

- [ ] **Step 5: Check rustfmt**

```bash
bazel build --config=rustfmt //:jsonnet_test_runner
```

Expected: No formatting errors. If there are, run `bazel run @rules_rust//:rustfmt` then re-check.

- [ ] **Step 6: Commit**

```bash
git add src/jsonnet_test_runner.rs
git commit -m "feat: auto-enable coverage and write to COVERAGE_OUTPUT_FILE"
```

---

### Task 3: Remove `coverage = "lcov"` from call sites and verify end-to-end

**Files:**
- Modify: `end2end/BUILD.bazel:29-35`, `end2end/BUILD.bazel:37-41`, `end2end/BUILD.bazel:43-47`

`coverage = "lcov"` previously triggered `--coverage` flag in the wrapper. Under `bazel coverage`, that flag is no longer needed — the runner auto-enables coverage via `$COVERAGE_OUTPUT_FILE`. Removing the attribute keeps call sites clean.

- [ ] **Step 1: Remove `coverage = "lcov"` from all three targets**

In `end2end/BUILD.bazel`, update the three `jsonnet_test` targets to remove `coverage = "lcov",`:

```python
jsonnet_test(
    name = "test_framework_integration_test",
    src = "test_framework_test.jsonnet",
    deps = [
        ":integration_lib",
    ],
)

jsonnet_test(
    name = "test_filter_framework_integration_test",
    src = "test_filter_framework_test.jsonnet",
)

jsonnet_test(
    name = "skip_test_framework_integration_test",
    src = "skip_test_framework_test.jsonnet",
)
```

- [ ] **Step 2: Verify all end2end tests still pass**

```bash
bazel test //end2end:test_framework_integration_test //end2end:test_filter_framework_integration_test //end2end:skip_test_framework_integration_test
```

Expected: All three tests PASSED.

- [ ] **Step 3: Run `bazel coverage` end-to-end**

```bash
bazel coverage //end2end:test_framework_integration_test --combined_report=lcov
```

Expected: Command completes successfully. Verify the combined report exists and contains `.jsonnet` source paths:

```bash
grep "SF:" bazel-out/_coverage/_coverage_report.dat | head -20
```

Expected output includes lines like:
```
SF:end2end/test_framework_test.jsonnet
SF:end2end/import_integration_test.libsonnet
```

- [ ] **Step 4: Commit**

```bash
git add end2end/BUILD.bazel
git commit -m "chore: remove coverage = lcov from jsonnet_test call sites"
```
