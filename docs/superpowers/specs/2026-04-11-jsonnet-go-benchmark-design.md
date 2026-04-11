# Add Go Jsonnet to Benchmark Suite

**Date:** 2026-04-11
**Status:** Approved

## Goal

Add `jsonnet_go` (the Go implementation of Jsonnet, v0.22.0) as a third benchmark participant alongside RapidJsonnet and Google Jsonnet (C++). Each benchmark file's markdown report will gain a third row for `GoJsonnet`.

## Background

The benchmark suite already compares RapidJsonnet against Google Jsonnet (C++) using hyperfine. The script accepts two implementation binaries as positional args followed by benchmark file paths. Adding Go Jsonnet follows the same pattern: a new 4th positional arg, a new conditional `case` block in the loop.

## Bazel Integration

`jsonnet_go` v0.22.0 is available on the Bazel Central Registry. It exposes a Go binary target:

```
@jsonnet_go//cmd/jsonnet:jsonnet
```

CLI interface: `jsonnet <file>` — identical to Google Jsonnet, no special flags needed.

## Skip List Analysis

Based on jrsonnet's benchmark results (v0.22.0-rc1, AMD Ryzen 9 9950X3D):

| Our benchmark file | Go result |
|---|---|
| `bench.01`–`bench.09.jsonnet` | All run successfully |
| `large_string_join.jsonnet` | 46ms — runs fine |
| `realistic_1.jsonnet` | ~3.2s/run — runs fine (slow) |
| `realistic_2.jsonnet` | ~3.2s/run — runs fine (slow) |
| `std_base64.jsonnet` | 8ms — runs fine |
| `std_foldl.jsonnet` | ~1s/run — runs fine |
| `comparison_array.jsonnet` | 77ms — runs fine |
| `comparison_primitives.jsonnet` | 892ms — runs fine |

**Go skip list: empty.** Go completes all our benchmark files without crashing.

The only known Go crash is `large_string_template.jsonnet` (OS stack exhaustion), which lives in the google/jsonnet `/perf_tests/` directory — **not covered by our `benchmarks/*.jsonnet` filegroup**. If `large_string_template.jsonnet` is ever added to `benchmarks/extra/`, it must be added to the Go skip list.

## Changes Required

### `MODULE.bazel`

Add after `bazel_dep(name = "jsonnet", version = "0.22.0")`:

```python
bazel_dep(name = "jsonnet_go", version = "0.22.0")
```

### `benchmarks/BUILD.bazel`

Add `@jsonnet_go//cmd/jsonnet:jsonnet` to `args` (as the 4th positional) and to `data`:

```python
sh_binary(
    name = "benchmark",
    srcs = ["run_benchmarks.sh"],
    args = [
        "$(location @hyperfine_bin//:hyperfine)",
        "$(location //:main)",
        "$(location @jsonnet//cmd:jsonnet)",
        "$(location @jsonnet_go//cmd/jsonnet:jsonnet)",
        "$(locations @jsonnet_test_suite_source//:benchmarks)",
        "$(locations //benchmarks:extra_benchmarks)",
    ],
    data = [
        "//:main",
        "@jsonnet_test_suite_source//:benchmarks",
        "//benchmarks:extra_benchmarks",
        "@hyperfine_bin//:hyperfine",
        "@jsonnet//cmd:jsonnet",
        "@jsonnet_go//cmd/jsonnet:jsonnet",
    ],
)
```

### `benchmarks/run_benchmarks.sh`

- Change `GOOGLE_BIN=$3; shift 3` to add `GO_BIN=$4; shift 4`
- Add a comment marking where to slot in future implementations
- Add a `GoJsonnet` conditional `case` block after the `GoogleJsonnet` block, with an empty skip list and a note about `large_string_template.jsonnet`

```bash
HYPERFINE_BIN=$1
MAIN_BIN=$2
GOOGLE_BIN=$3
GO_BIN=$4
# To add another implementation: add it here as $5 and increment shift to 5
shift 4

...

    # Conditionally benchmark GoogleJsonnet (C++)
    case "$filename" in
        "bench.07.jsonnet" | \
        "bench.09.jsonnet" | \
        "realistic_1.jsonnet" | \
        "realistic_2.jsonnet" | \
        "std_base64.jsonnet" | \
        "comparison_array.jsonnet" | \
        "comparison_primitives.jsonnet")
            echo "Skipping GoogleJsonnet for $filename (known performance/stability issue)"
            ;;
        *)
            hyperfine_args+=("-n" "GoogleJsonnet: $filename" "$GOOGLE_BIN $item")
            ;;
    esac

    # Conditionally benchmark GoJsonnet
    # Note: large_string_template.jsonnet crashes Go (OS stack exhaustion) — if it's
    # ever added to benchmarks/extra/, add it to the skip list below.
    case "$filename" in
        *) hyperfine_args+=("-n" "GoJsonnet: $filename" "$GO_BIN $item") ;;
    esac
```

## Design Decisions

**Why positional arg pattern (not extensible flags)?** The current suite has two reference implementations. Bash flag parsing adds complexity not yet warranted. The `# To add another implementation` comment marks where to extend when a fifth binary appears.

**Why an empty skip list for Go?** All files in our suite complete successfully for Go. Go is slower than Rust on every benchmark (2x–530x) but never hangs or crashes on our files. This is confirmed by jrsonnet's published benchmark data.

**Why `large_string_template.jsonnet` is not in the skip list?** It's in the google/jsonnet `/perf_tests/` directory, which is not covered by our `benchmarks/*.jsonnet` filegroup. It simply isn't a benchmark we run.

## Output

Each `benchmark-results/*-results.md` gains a third row:

```
| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|---|---|---|---|---|
| RapidJsonnet: bench.01.jsonnet  | ...  | ... | ... | 1.00 |
| GoogleJsonnet: bench.01.jsonnet | ...  | ... | ... | X.XX |
| GoJsonnet: bench.01.jsonnet     | ...  | ... | ... | X.XX |
```

## Files Changed

| File | Change |
|---|---|
| `MODULE.bazel` | Add `bazel_dep(name = "jsonnet_go", version = "0.22.0")` |
| `benchmarks/BUILD.bazel` | Add `@jsonnet_go//cmd/jsonnet:jsonnet` to args and data |
| `benchmarks/run_benchmarks.sh` | Add `GO_BIN=$4`, `shift 4`, GoJsonnet case block |
