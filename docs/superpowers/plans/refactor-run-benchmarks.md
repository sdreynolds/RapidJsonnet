# Refactor `run_benchmarks.sh` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor `benchmarks/run_benchmarks.sh` to use a Bash array for `hyperfine` arguments, allowing conditional skipping of GoogleJsonnet for specific benchmark files.

**Architecture:** Use Bash array to store `hyperfine` arguments and a `case` statement to conditionally add GoogleJsonnet benchmark to the array based on the filename.

**Tech Stack:** Bash script, Hyperfine.

---

### Task 1: Refactor `benchmarks/run_benchmarks.sh`

**Files:**
- Modify: `benchmarks/run_benchmarks.sh`

- [ ] **Step 1: Replace the loop body with the array-based logic**

```bash
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
```

- [ ] **Step 2: Commit the change**

```bash
git add benchmarks/run_benchmarks.sh
git commit -m "perf: skip GoogleJsonnet for slow/broken benchmarks"
```

### Task 2: Verify the Logic

**Files:**
- None (just running commands)

- [ ] **Step 1: Run a small subset of benchmarks**

Run: `bazel run //benchmarks:benchmark -- bench.01.jsonnet bench.07.jsonnet`
Expected: 
- `bench.01.jsonnet` runs both RapidJsonnet and GoogleJsonnet.
- `bench.07.jsonnet` prints "Skipping GoogleJsonnet for bench.07.jsonnet" and runs ONLY RapidJsonnet.

- [ ] **Step 2: Verify the output files**

Run: `cat benchmark-results/bench.01.jsonnet-results.md`
Expected: Check for 2 rows in table (RapidJsonnet and GoogleJsonnet).

Run: `cat benchmark-results/bench.07.jsonnet-results.md`
Expected: Check for 1 row in table (Only RapidJsonnet).
