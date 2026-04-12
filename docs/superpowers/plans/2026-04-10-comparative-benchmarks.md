# Comparative Benchmarking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `@jsonnet//cmd:jsonnet` into the benchmark target so each benchmark file produces a side-by-side hyperfine markdown report comparing RapidJsonnet and Google Jsonnet.

**Architecture:** The `sh_binary` benchmark target already receives hyperfine and RapidJsonnet as positional args. We add the Google Jsonnet binary as a third arg, update the shell script to accept it, and replace the single-command hyperfine call with two named commands per benchmark file. Output goes to `$BUILD_WORKSPACE_DIRECTORY/benchmark-results/`.

**Tech Stack:** Bazel `sh_binary`, hyperfine, bash

---

## Already Done (do not redo)

- `MODULE.bazel` — `bazel_dep(name = "jsonnet", version = "0.22.0")` added
- `third_party_test_suite/extension.bzl` — bumped to v0.22.0 with correct sha256

## File Map

| File | Change |
|---|---|
| `benchmarks/BUILD.bazel` | Add `@jsonnet//cmd:jsonnet` to `data` and `args` |
| `benchmarks/run_benchmarks.sh` | Accept third arg, two-command hyperfine call, `$BUILD_WORKSPACE_DIRECTORY` output |
| `.gitignore` | Add `benchmark-results/` |

---

### Task 1: Update `benchmarks/BUILD.bazel`

**Files:**
- Modify: `benchmarks/BUILD.bazel`

- [ ] **Step 1: Add `@jsonnet//cmd:jsonnet` to the `sh_binary` target**

Replace the entire `benchmark` target in `benchmarks/BUILD.bazel` with:

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

- [ ] **Step 2: Verify the target builds**

Run:
```bash
bazel build //benchmarks:benchmark
```

Expected: `INFO: Build completed successfully`

- [ ] **Step 3: Commit**

```bash
git add benchmarks/BUILD.bazel
git commit -m "feat: add google/jsonnet binary to benchmark target"
```

---

### Task 2: Update `benchmarks/run_benchmarks.sh`

**Files:**
- Modify: `benchmarks/run_benchmarks.sh`

- [ ] **Step 1: Rewrite the script**

Replace the entire contents of `benchmarks/run_benchmarks.sh` with:

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

- [ ] **Step 2: Run the benchmark end-to-end**

```bash
bazel run //benchmarks:benchmark
```

Expected:
- Output contains `Running benchmarks using hyperfine...`
- Output contains `=== BENCHMARK RESULTS SAVED TO .../benchmark-results/ ===`
- A `benchmark-results/` directory appears in the workspace root containing one `*-results.md` file per benchmark file
- Each markdown file has two rows — one for `RapidJsonnet` and one for `GoogleJsonnet`

Spot-check one file to confirm two-row output:
```bash
cat benchmark-results/bench.01.jsonnet-results.md
```

Expected: a markdown table with `RapidJsonnet: bench.01.jsonnet` and `GoogleJsonnet: bench.01.jsonnet` rows.

- [ ] **Step 3: Commit**

```bash
git add benchmarks/run_benchmarks.sh
git commit -m "feat: compare RapidJsonnet vs Google Jsonnet with hyperfine"
```

---

### Task 3: Update `.gitignore`

**Files:**
- Modify: `.gitignore`

- [ ] **Step 1: Add `benchmark-results/` to `.gitignore`**

Append to `.gitignore`:

```
/benchmark-results
```

- [ ] **Step 2: Verify the directory is ignored**

```bash
git status
```

Expected: `benchmark-results/` does not appear in untracked files.

- [ ] **Step 3: Commit**

```bash
git add .gitignore
git commit -m "chore: ignore benchmark-results output directory"
```
