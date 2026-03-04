# Project Guidelines

This project uses `BUILD.bazel` and `bazel` to build the project.
- `bazel test //...` runs all the tests.
- To run code formatting: `bazel run @rules_rust//:rustfmt`.
- To run a jsonnet file through main run `bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/<the-name-of-the-file>`. This is because the binary is placed into the bazel sandbox environment.
