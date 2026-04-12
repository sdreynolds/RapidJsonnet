# Comparative Benchmarking: RapidJsonnet vs Google Jsonnet

**Date:** 2026-04-10
**Status:** Approved

## Goal

Add side-by-side performance comparison between RapidJsonnet and the reference Google Jsonnet implementation (v0.22.0) to the existing hyperfine benchmark suite. Each benchmark file produces a single markdown report with one row per implementation, written to `$BUILD_WORKSPACE_DIRECTORY/benchmark-results/`.

## What Was Already Done

- `MODULE.bazel` — `bazel_dep(name = "jsonnet", version = "0.22.0")` added; provides `@jsonnet//cmd:jsonnet`
- `third_party_test_suite/extension.bzl` — bumped from v0.20.0 to v0.22.0 (sha256: `5914b9904d97efa662d919519cef1a14e4132bfddddaeed8b061b4a8af628f8d`)

## Source of Truth for Each Artifact

| Artifact | Source |
|---|---|
| `jsonnet` binary (reference impl) | `bazel_dep` → `@jsonnet//cmd:jsonnet` |
| Test `.jsonnet` / `.golden` files | `http_archive` in `third_party_test_suite/extension.bzl` with custom BUILD |
| Benchmark `.jsonnet` files | Same `http_archive` + `//benchmarks:extra_benchmarks` |

The BCR module for `jsonnet` uses `//visibility:private` on `test_suite/BUILD` and exposes no filegroup targets for test or benchmark files. The `http_archive` with our custom `third_party_test_suite/jsonnet.BUILD.bazel` is the only way to surface those filegroups.

## Changes Required

### `benchmarks/BUILD.bazel`

Add `@jsonnet//cmd:jsonnet` to the `sh_binary` data deps and pass its location as a third positional arg (after hyperfine and RapidJsonnet):

```python
sh_binary(
    name = "benchmark",
    srcs = ["run_benchmarks.sh"],
    args = [
        "$(location @hyperfine_bin//:hyperfine)",
        "$(location //:main)",
        "$(location @jsonnet//cmd:jsonnet)",
        "$(locations @jsonnet_test_suite_source//:benchmarks)",
        "$(locations //benchmarks:extra_benchmarks)",
    ],
    data = [
        "//:main",
        "@jsonnet_test_suite_source//:benchmarks",
        "//benchmarks:extra_benchmarks",
        "@hyperfine_bin//:hyperfine",
        "@jsonnet//cmd:jsonnet",
    ],
)
```

### `benchmarks/run_benchmarks.sh`

Accept a third positional arg `$GOOGLE_BIN`. Replace the single-command hyperfine call with a two-command named comparison. Output directory is `$BUILD_WORKSPACE_DIRECTORY/benchmark-results/`, created on first run.

```bash
#!/bin/bash
set -e

HYPERFINE_BIN=$1
MAIN_BIN=$2
GOOGLE_BIN=$3
shift 3

BENCHMARKS=("$@")

if [ ${#BENCHMARKS[@]} -eq 0 ]; then
    echo "No benchmarks provided!"
    exit 1
fi

OUT_DIR="${BUILD_WORKSPACE_DIRECTORY:-.}/benchmark-results"
mkdir -p "$OUT_DIR"

echo "Running benchmarks using hyperfine..."
for item in "${BENCHMARKS[@]}"; do
    filename=$(basename "$item")

    $HYPERFINE_BIN -w 3 \
      --export-markdown "$OUT_DIR/$filename-results.md" \
      -n "RapidJsonnet: $filename" "$MAIN_BIN -q $item" \
      -n "GoogleJsonnet: $filename" "$GOOGLE_BIN $item"
done

echo "=== BENCHMARK RESULTS SAVED TO $OUT_DIR/ ==="
```

### `.gitignore`

Add `benchmark-results/` so generated markdown files are not committed.

## Design Decisions

**Why not `-L`?** The two implementations have different flag interfaces (`-q` for RapidJsonnet, nothing for Google Jsonnet). Using `-L` would require wrapper scripts in a temp directory to normalize the invocation. Passing two named commands directly to hyperfine produces an identical side-by-side markdown table without the complexity.

**Why `$BUILD_WORKSPACE_DIRECTORY`?** This variable is set automatically by Bazel for all `bazel run` targets and points to the workspace root — the idiomatic equivalent of `$TEST_UNDECLARED_OUTPUTS_DIR` for `sh_binary`. Falls back to `.` when invoked outside of Bazel.

**Why not merge test files into `bazel_dep`?** The BCR module's `test_suite/BUILD` is `//visibility:private` with no public filegroup targets. The `http_archive` with our custom BUILD file is the only way to expose the filegroups our test targets reference.

## Output

For each benchmark file, one markdown file at `benchmark-results/$filename-results.md` with a two-row table:

```
| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|---|---|---|---|---|
| RapidJsonnet: bench.01.jsonnet | ... | ... | ... | 1.00 |
| GoogleJsonnet: bench.01.jsonnet | ... | ... | ... | 1.23 |
```

## Files Changed

| File | Change |
|---|---|
| `MODULE.bazel` | Add `bazel_dep(name = "jsonnet", version = "0.22.0")` *(done)* |
| `third_party_test_suite/extension.bzl` | Bump to v0.22.0 *(done)* |
| `benchmarks/BUILD.bazel` | Add `@jsonnet//cmd:jsonnet` to data + args |
| `benchmarks/run_benchmarks.sh` | Accept third arg, two-command hyperfine call, write to `$BUILD_WORKSPACE_DIRECTORY/benchmark-results/` |
| `.gitignore` | Add `benchmark-results/` |
