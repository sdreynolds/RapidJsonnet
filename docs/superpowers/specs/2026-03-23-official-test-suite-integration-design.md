# Spec: Official Jsonnet Test Suite Integration

## Status
Approved

## Context
The current `test_suite` contains a subset (37 files) of the official Jsonnet test suite. We want to leverage the full official suite from `google/jsonnet` (v0.20.0) without manually copying files into the repo. We will use Bazel's Bzlmod with an overlay to fetch the official repo and run its tests using our `//:main` interpreter.

## Goals
- Fetch official `google/jsonnet` v0.20.0 as an external dependency.
- Use an overlay to define how the external files are exposed.
- Generate `sh_test` targets dynamically from a manifest of test names.
- Replace local copies in `test_suite/` with references to external files.

## Architecture

### 1. External Repository (`third_party_test_suite`)
We will create a `third_party_test_suite` directory to house the Bzlmod extension and the overlay `BUILD` file.

- **`extension.bzl`**: A module extension that uses `http_archive` to fetch `google/jsonnet`. It will apply `jsonnet.BUILD.bazel` as an overlay.
- **`jsonnet.BUILD.bazel`**: The overlay file. It defines `filegroup` targets for `.jsonnet`, `.golden`, and support files (like `lib/`).

### 2. Test Manifest (`test_suite/test_manifest.bzl`)
Since Bazel cannot `glob` external files at analysis time in a local `BUILD` file, we will maintain a `TESTS` list in a `.bzl` file. This also provides an easy way to "xfail" or skip tests if needed in the future.

### 3. Test Generation (`test_suite/BUILD.bazel`)
The build file will loop over the manifest and create an `sh_test` for each entry. It will point to `run_test.sh` as the runner.

### 4. Test Runner (`test_suite/run_test.sh`)
The script will be updated to handle the paths of external data files provided by Bazel.

## Components

### `third_party_test_suite/extension.bzl`
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

### `third_party_test_suite/jsonnet.BUILD.bazel`
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

# Individual file exports for manifest-based access
exports_files(glob(["test_suite/**"]))
```

## Implementation Plan
1. Create `third_party_test_suite/` directory and files.
2. Update `MODULE.bazel` to register the extension.
3. Create `test_suite/test_manifest.bzl` with the list of tests from v0.20.0.
4. Update `test_suite/BUILD.bazel` to use the manifest and external repo.
5. Update `test_suite/run_test.sh` to ensure path compatibility.
6. Verify by running `bazel test //test_suite/...`.
7. Cleanup: Remove local `.jsonnet` and `.golden` files from `test_suite/`.

## Verification
- `bazel test //test_suite/...` should pass for all official tests.
