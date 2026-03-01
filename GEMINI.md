# Project Guidelines

This project uses `BUILD.bazel` and `bazel` to build the project.
- `bazel test //...` runs all the tests.
- To run code formatting: `bazel run @rules_rust//:rustfmt`.
