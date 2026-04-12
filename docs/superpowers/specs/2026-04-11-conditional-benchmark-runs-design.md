# Conditional Benchmark Runs for GoogleJsonnet

**Date:** 2026-04-11
**Status:** Proposed

## Goal

Update `benchmarks/run_benchmarks.sh` to skip running the reference Google Jsonnet implementation (`GOOGLE_BIN`) on specific benchmark files that are known to fail, be excessively slow, or consume unreasonable amounts of memory in the C++ implementation. This ensures that the benchmarking suite completes in a reasonable time and provides stable results for RapidJsonnet.

## Research Findings

The `jrsonnet` project's benchmarks (using v0.22.0-rc1) indicate that several tests are problematic for the C++ implementation (Google Jsonnet). Based on those findings and user requirements, the following files should skip `GOOGLE_BIN`:

| Benchmark File | Reason |
| :--- | :--- |
| `bench.07.jsonnet` | User requested skip (Lazy array) |
| `bench.09.jsonnet` | Too slow for C++ (String strips) |
| `realistic_1.jsonnet` | Too slow for C++ (hours) |
| `realistic_2.jsonnet` | Too slow for C++ (hours) |
| `std_base64.jsonnet` | Too slow for C++ (hours) |
| `comparison_array.jsonnet` | Too slow for C++ (hours) |
| `comparison_primitives.jsonnet` | High RAM usage (192GB) |

## Proposed Changes

### `benchmarks/run_benchmarks.sh`

Refactor the `hyperfine` invocation to use a dynamic argument array. This allows for conditional addition of the `GOOGLE_BIN` command based on the filename.

```bash
#!/bin/bash
# ... existing setup ...

for item in "${BENCHMARKS[@]}"; do
    filename=$(basename "$item")

    # Always benchmark RapidJsonnet
    hyperfine_args=(
      "-w" "3"
      "--export-markdown" "$OUT_DIR/$filename-results.md"
      "-n" "RapidJsonnet: $filename" "$MAIN_BIN -q $item"
    )

    # Conditionally benchmark GoogleJsonnet
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

    $HYPERFINE_BIN "${hyperfine_args[@]}"
done
```

## Design Decisions

- **Why an array?** Bash arrays are the safest way to pass multi-part arguments to a command while preserving spaces and preventing unwanted word splitting.
- **Why a `case` statement?** A `case` statement is more readable and easier to maintain than a long `if` condition when checking against a list of static strings.
- **Why skip `bench.09.jsonnet`?** The `jrsonnet` results show "No results for C++, too slow, takes hours" for "String strips", which matches `bench.09.jsonnet`.

## Verification Plan

1. Run the benchmark suite: `bazel run //benchmarks:benchmark`.
2. Observe the output to confirm that `Skipping GoogleJsonnet for ...` is printed for the targeted files.
3. Verify that the generated markdown files in `benchmark-results/` for skipped files only contain a single row for RapidJsonnet.
4. Verify that other benchmarks (e.g., `bench.01.jsonnet`) still run both implementations.
