# Build Instructions

This project uses Bazel to build Rust code. Follow these instructions to build and test the project.

## Prerequisites

- [Bazel](https://bazel.build/) installed on your system
- Rust toolchain (handled by Bazel rules)

## Building the Project

To build all targets:
```bash
bazel build //...
```

To build a specific target:
```bash
bazel build //path/to:target_name
```

## Running Tests

To run all tests:
```bash
bazel test //...
```

To run tests for a specific target:
```bash
bazel test //path/to:test_target
```

## Adding Source Code

When adding new Rust source code, you need to include it in the appropriate Bazel rules:

- **For executables**: Add to `rust_binary` rules in `BUILD.bazel`
- **For libraries**: Add to `rust_library` rules in `BUILD.bazel`

Example `rust_library` rule:
```python
rust_library(
    name = "my_lib",
    srcs = ["src/lib.rs"],
    deps = [
        # Dependencies here
    ],
)
```

Example `rust_binary` rule:
```python
rust_binary(
    name = "my_binary",
    srcs = ["src/main.rs"],
    deps = [
        ":my_lib",
        # Other dependencies
    ],
)
```

## Common Commands

- `bazel build //...` - Build everything
- `bazel test //...` - Run all tests
- `bazel clean` - Clean build artifacts
- `bazel query //...` - List all targets