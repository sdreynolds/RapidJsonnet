# Conditional Benchmark Runs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Update `benchmarks/run_benchmarks.sh` to skip `GoogleJsonnet` for specific slow/broken files using a dynamic Bash array for `hyperfine` arguments.

**Architecture:** Use a Bash array to store `hyperfine` flags and commands, conditionally appending the `GoogleJsonnet` command based on a `case` statement that matches the benchmark filename.

**Tech Stack:** Bash, Hyperfine, Bazel

---

### Task 1: Refactor `run_benchmarks.sh` to use Argument Array

**Files:**
- Modify: `benchmarks/run_benchmarks.sh`

- [ ] **Step 1: Modify `benchmarks/run_benchmarks.sh` to use an array for hyperfine arguments**

```bash
<<<<
    $HYPERFINE_BIN -w 3 \
      --export-markdown "$OUT_DIR/$filename-results.md" \
      -n "RapidJsonnet: $filename" "$MAIN_BIN -q $item" \
      -n "GoogleJsonnet: $filename" "$GOOGLE_BIN $item"
====
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
>>>>
```

- [ ] **Step 2: Run a small subset of benchmarks to verify the logic**

Run: `bazel run //benchmarks:benchmark -- bench.01.jsonnet bench.07.jsonnet`
Expected: 
- `bench.01.jsonnet` runs both RapidJsonnet and GoogleJsonnet.
- `bench.07.jsonnet` prints "Skipping GoogleJsonnet for bench.07.jsonnet" and runs ONLY RapidJsonnet.

- [ ] **Step 3: Verify the output files**

Run: `cat benchmark-results/bench.01.jsonnet-results.md` (Check for 2 rows in table)
Run: `cat benchmark-results/bench.07.jsonnet-results.md` (Check for 1 row in table)

- [ ] **Step 4: Commit the change**

```bash
git add benchmarks/run_benchmarks.sh
git commit -m "perf: skip GoogleJsonnet for slow/broken benchmarks"
```
