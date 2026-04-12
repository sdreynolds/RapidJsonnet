# Official Jsonnet Test Suite Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate the official `google/jsonnet` (v0.20.0) test suite using a Bzlmod overlay to replace local file copies.

**Architecture:** Use a module extension in `third_party_test_suite/` to fetch the external repo with an overlay `BUILD` file. Generate tests dynamically in `test_suite/BUILD.bazel` from a manifest file.

**Tech Stack:** Bazel (Bzlmod), Bash.

---

### Task 1: Setup Overlay and Extension

**Files:**
- Create: `third_party_test_suite/extension.bzl`
- Create: `third_party_test_suite/jsonnet.BUILD.bazel`
- Create: `third_party_test_suite/BUILD.bazel`
- Modify: `MODULE.bazel`

- [ ] **Step 1: Create `third_party_test_suite/BUILD.bazel`**
  ```python
  package(default_visibility = ["//visibility:public"])
  exports_files(["jsonnet.BUILD.bazel"])
  ```

- [ ] **Step 2: Create `third_party_test_suite/extension.bzl`**
  ```python
  load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")

  def _jsonnet_tests_impl(ctx):
      http_archive(
          name = "jsonnet_test_suite_source",
          urls = ["https://github.com/google/jsonnet/archive/refs/tags/v0.20.0.tar.gz"],
          sha256 = "77bd269073807731f6b11ff8d7c03e9065aafb8e4d038935deb388325e52511b",
          strip_prefix = "jsonnet-0.20.0",
          build_file = "//third_party_test_suite:jsonnet.BUILD.bazel",
      )

  jsonnet_tests = module_extension(implementation = _jsonnet_tests_impl)
  ```

- [ ] **Step 3: Create `third_party_test_suite/jsonnet.BUILD.bazel` (Overlay)**
  ```python
  package(default_visibility = ["//visibility:public"])

  filegroup(
      name = "test_files",
      srcs = glob(["test_suite/*.jsonnet"]),
  )

  filegroup(
      name = "golden_files",
      srcs = glob(["test_suite/*.jsonnet.golden"]),
  )

  filegroup(
      name = "support_files",
      srcs = glob([
          "test_suite/lib/**",
          "test_suite/this_file/**",
          "test_suite/*.jsonnet.in",
      ]),
  )

  exports_files(glob(["test_suite/**"]))
  ```

- [ ] **Step 4: Update `MODULE.bazel`**
  Add to the end:
  ```python
  jsonnet_tests = use_extension("//third_party_test_suite:extension.bzl", "jsonnet_tests")
  use_repo(jsonnet_tests, "jsonnet_test_suite_source")
  ```

- [ ] **Step 5: Verify repository fetch**
  Run: `bazel query @jsonnet_test_suite_source//test_suite:test_files`
  Expected: Success, listing files like `@jsonnet_test_suite_source//test_suite:arith_bool.jsonnet`.

- [ ] **Step 6: Commit**
  ```bash
  git add third_party_test_suite/ MODULE.bazel
  git commit -m "chore: setup official jsonnet test suite external repo"
  ```

### Task 2: Manifest and Test Generation

**Files:**
- Create: `test_suite/test_manifest.bzl`
- Modify: `test_suite/BUILD.bazel`
- Modify: `test_suite/run_test.sh`

- [ ] **Step 1: Create `test_suite/test_manifest.bzl`**
  Define a list of test names and whether they have golden files.
  ```python
  # Initial list from local knowledge, will be expanded in Task 3
  TESTS = [
      ("arith_bool", True),
      ("arith_float", False),
      ("arith_string", False),
      ("array", False),
      ("array_comparison", True),
      ("array_comparison2", True),
      ("assert", False),
      ("binary", False),
      ("comments", False),
      ("condition", False),
      ("digitsep", True),
      ("format", False),
      ("functions", False),
      ("invariant", False),
      ("local", False),
      ("object", False),
      ("operators", False),
      ("precedence", False),
      ("recursive_import", False),
      ("recursive_import_error", False),
      ("string_comprehension", False),
      ("strings", False),
      ("tail_recursion", False),
      ("unicode", False),
  ]
  ```

- [ ] **Step 2: Update `test_suite/BUILD.bazel`**
  ```python
  load("@rules_shell//shell:sh_test.bzl", "sh_test")
  load(":test_manifest.bzl", "TESTS")

  SUPPORT_FILES = [
      "@jsonnet_test_suite_source//test_suite:support_files",
  ]

  [
      sh_test(
          name = t[0].replace(".", "_") + "_test",
          srcs = ["run_test.sh"],
          args = [
              "$(location //:main)",
              "$(location @jsonnet_test_suite_source//test_suite:%s.jsonnet)" % t[0],
          ] + (["$(location @jsonnet_test_suite_source//test_suite:%s.jsonnet.golden)" % t[0]] if t[1] else []),
          data = [
              "//:main",
              "@jsonnet_test_suite_source//test_suite:%s.jsonnet" % t[0],
          ] + (["@jsonnet_test_suite_source//test_suite:%s.jsonnet.golden" % t[0]] if t[1] else []) + SUPPORT_FILES,
      )
      for t in TESTS
  ]
  ```

- [ ] **Step 3: Update `test_suite/run_test.sh`**
  Ensure it handles external paths correctly. Bazel paths for external repos often start with `external/`.

- [ ] **Step 4: Verify initial tests pass**
  Run: `bazel test //test_suite/...`
  Expected: PASS for the subset in manifest.

- [ ] **Step 5: Commit**
  ```bash
  git add test_suite/test_manifest.bzl test_suite/BUILD.bazel test_suite/run_test.sh
  git commit -m "feat: use official test suite manifest in BUILD"
  ```

### Task 3: Full Suite Transition and Cleanup

- [ ] **Step 1: Discover all tests in external repo**
  Run: `ls $(bazel info output_base)/external/jsonnet_test_suite_source/test_suite/*.jsonnet`
  Update `test_manifest.bzl` with the full list (approx 64 files).

- [ ] **Step 2: Run all official tests**
  Run: `bazel test //test_suite/...`
  Expected: PASS

- [ ] **Step 3: Remove local test files**
  Run: `rm test_suite/*.jsonnet test_suite/*.golden`

- [ ] **Step 4: Final verification**
  Run: `bazel test //test_suite/...`
  Expected: PASS (now running purely against external source)

- [ ] **Step 5: Commit**
  ```bash
  git add test_suite/
  git commit -m "cleanup: remove local test files, fully transitioned to official suite"
  ```
