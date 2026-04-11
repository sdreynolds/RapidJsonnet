# Add Go Jsonnet to Benchmark Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `jsonnet_go` v0.22.0 as a third benchmark participant so each hyperfine report includes a `GoJsonnet` row alongside `RapidJsonnet` and `GoogleJsonnet`.

**Architecture:** Three sequential file edits — `MODULE.bazel` pulls in the BCR dep, `benchmarks/BUILD.bazel` wires the Go binary as a 4th positional arg, and `benchmarks/run_benchmarks.sh` reads that arg and appends a `GoJsonnet` command to each hyperfine invocation. No new files are created.

**Tech Stack:** Bazel (MODULE.bazel, BUILD.bazel), bash, hyperfine, `@jsonnet_go//cmd/jsonnet:jsonnet` from the Bazel Central Registry.

---

### Task 1: Add jsonnet_go to MODULE.bazel

**Files:**
- Modify: `MODULE.bazel`

Context: `MODULE.bazel` declares all external Bazel dependencies. `jsonnet_go` v0.22.0 is on the Bazel Central Registry and exposes `@jsonnet_go//cmd/jsonnet:jsonnet`. The existing `jsonnet` dep (C++ impl) is already present — add `jsonnet_go` directly after it.

- [ ] **Step 1: Open MODULE.bazel and find the jsonnet dep**

Read `MODULE.bazel`. Locate this line (around line 75):
```
bazel_dep(name = "jsonnet", version = "0.22.0")
```

- [ ] **Step 2: Add jsonnet_go dep immediately after it**

The file currently reads:
```
bazel_dep(name = "rules_shell", version = "0.6.1")
bazel_dep(name = "jsonnet", version = "0.22.0")

jsonnet_tests = use_extension("//third_party_test_suite:extension.bzl", "jsonnet_tests")
```

Change it to:
```
bazel_dep(name = "rules_shell", version = "0.6.1")
bazel_dep(name = "jsonnet", version = "0.22.0")
bazel_dep(name = "jsonnet_go", version = "0.22.0")

jsonnet_tests = use_extension("//third_party_test_suite:extension.bzl", "jsonnet_tests")
```

- [ ] **Step 3: Verify Bazel resolves the new dep**

Run:
```bash
bazel build @jsonnet_go//cmd/jsonnet:jsonnet
```

Expected: build succeeds and produces a `jsonnet` binary. This may download Go toolchain on first run — give it time.

- [ ] **Step 4: Commit**

```bash
git add MODULE.bazel
git commit -m "feat: add jsonnet_go v0.22.0 bazel_dep for benchmark suite"
```

---

### Task 2: Wire Go binary into benchmarks/BUILD.bazel

**Files:**
- Modify: `benchmarks/BUILD.bazel`

Context: The `benchmark` sh_binary passes all binary and file paths as positional args to `run_benchmarks.sh` via the `args` attribute. The script currently reads: `$1=hyperfine`, `$2=RapidJsonnet`, `$3=GoogleJsonnet`, `$4...$N=benchmark files`. We add `$4=GoJsonnet` and shift the benchmark files to `$5...$N`.

- [ ] **Step 1: Open benchmarks/BUILD.bazel**

The file currently reads:
```python
load("@rules_shell//shell:sh_binary.bzl", "sh_binary")

filegroup(
    name = "extra_benchmarks",
    srcs = glob(["extra/*.jsonnet"]),
)

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

- [ ] **Step 2: Add Go binary as the 4th positional arg**

Replace the entire `sh_binary` block with:
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

- [ ] **Step 3: Verify the build target resolves**

Run:
```bash
bazel build //benchmarks:benchmark
```

Expected: build succeeds. Do NOT run the benchmark — it takes over an hour.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/BUILD.bazel
git commit -m "feat: add jsonnet_go binary to benchmark BUILD target"
```

---

### Task 3: Update run_benchmarks.sh to read and use GO_BIN

**Files:**
- Modify: `benchmarks/run_benchmarks.sh`

Context: The script currently reads 3 binary args then shifts. We add `GO_BIN` as the 4th arg (shifting to `shift 4`) and append a new `case` block that adds a `GoJsonnet` command to every hyperfine invocation. The Go skip list is empty — all our benchmark files run successfully for Go. The one known Go crash (`large_string_template.jsonnet`) is not in our suite.

- [ ] **Step 1: Open benchmarks/run_benchmarks.sh**

The file currently reads:
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

echo "=== BENCHMARK RESULTS SAVED TO $OUT_DIR/ ==="
```

- [ ] **Step 2: Replace the file with the updated version**

Write the complete updated file:
```bash
#!/bin/bash
set -e

HYPERFINE_BIN=$1
MAIN_BIN=$2
GOOGLE_BIN=$3
GO_BIN=$4
# To add another implementation: add it here as $5 and increment shift to 5
shift 4

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

    # Always benchmark RapidJsonnet
    hyperfine_args=(
      "-w" "3"
      "--export-markdown" "$OUT_DIR/$filename-results.md"
      "-n" "RapidJsonnet: $filename" "$MAIN_BIN -q $item"
    )

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

    $HYPERFINE_BIN "${hyperfine_args[@]}"
done

echo "=== BENCHMARK RESULTS SAVED TO $OUT_DIR/ ==="
```

- [ ] **Step 3: Verify the benchmark target still builds**

Run:
```bash
bazel build //benchmarks:benchmark
```

Expected: build succeeds. This confirms Bazel correctly packages the updated script with all four binaries.

- [ ] **Step 4: Commit**

```bash
git add benchmarks/run_benchmarks.sh
git commit -m "feat: add GoJsonnet as third benchmark participant"
```
