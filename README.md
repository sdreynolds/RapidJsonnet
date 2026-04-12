# RapidJsonnet
RapidJsonnet is a personal experiment with AI against a well-defined spec using Rust and Bazel. The result is a competitive [Jsonnet interpreter](https://jsonnet.org/) that ships with two novel features -- unit test runner with coverage and ahead of time compilation. It is not battle-tested in production; use with caution and test thoroughly before replacing existing implementations.

## Status
This is an experimental project. It likely is has bugs in implementation and in the standard library that ships with it. Use with caution.


## Performance
The core of RapidJsonnet is to compile jsonnet into a performant byte code and use that to manifest the json (and other outputs). The repo ships with a bazel target that can be run to compare RapidJsonnet against:

- [Original Jsonnet from Google](https://github.com/google/jsonnet)
- [Jsonnet in Go from Google](https://github.com/google/go-jsonnet/)

These were easy to add to the bazel automation as they are published to [Bazel Central](https://registry.bazel.build/) and can be added with little fuss. The benchmarks use [Hyperfine](https://github.com/sharkdp/hyperfine) which is downloaded as part of the build process. To run the tests execute:

``` bash
bazel run -c opt //benchmarks:benchmark
```

### Performance Where RapidJsonnet Wins (with `-c opt`)

| Benchmark               | RapidJsonnet | vs GoJsonnet                       |
|-------------------------|--------------|------------------------------------|
| `gen_big_object`        | 60ms         | **fastest** (Go 66ms, Google 68ms) |
| `std_foldl`             | 318ms        | **11.5× faster** than Go           |
| `std_reverse`           | 230ms        | **2× faster** than Go              |
| `std_base64`            | 6.8ms        | **3.3× faster** than Go            |
| `std_base64Decode`      | 8.9ms        | **2.5× faster** than Go            |
| `std_base64DecodeBytes` | 178ms        | **2.1× faster** than Go            |
| `realistic_1`           | 186ms        | **63× faster** than Go             |
| `realistic_2`           | 3.0s         | **3.2× faster** than Go            |
| `bench.04`              | 1.26s        | **11.2× faster** than Go           |
| `bench.08`              | 1.8ms        | **1.7× faster** than Go            |
| `bench.09`              | 2.1ms        | **6.6× faster** than Go            |

### Performance: Where RapidJsonnet Lags

| Benchmark               | RapidJsonnet | GoJsonnet | Gap                                          | What it tests                                                                                          |
|-------------------------|--------------|-----------|----------------------------------------------|--------------------------------------------------------------------------------------------------------|
| `comparison_primitives` | 6.683s       | 2.213s    | **3.0× slower**                              | `[i < j for i in range(1000) for j in range(1000)]` — 1M primitive comparisons in nested comprehension |
| `bench.02`              | 3.800s       | 1.744s    | **2.18× slower**                             | OO Fibonacci via object extension — recursive object creation + field access                           |
| `comparison_array`      | 296ms        | 159ms     | **1.86× slower**                             | `long_array + [1] < long_array + [2]` — array concat + lexicographic comparison of 1M elements         |
| `bench.03`              | 1.165s       | 0.755s    | **1.54× slower** (5.3× vs GoogleJsonnet C++) | Simple recursive `fibonacci(25)` — pure function call overhead


## Getting Started
To get started, have a [bazel](https://bazel.build/) version of 8.3.1 or greater. Bazel handles all the rest of the required dependecies including rust tool chain, C++ tool chain and [hyperfine cli](https://github.com/sharkdp/hyperfine) for benchmarks. It is recommended to use [Bazelisk](https://github.com/bazelbuild/bazelisk) as a user friendly wrapper around Bazel. Run `bazel run //:main` to jump right into a Jsonnet REPL and run `bazel run //:main -- --quiet file.jsonnet` to have it execute directly on an existing file. The `--quiet` flag prevents the diagnostic output from the virtual machine and compiler and just returns the resulting json output.

## Conformance to Jsonnet Spec
The implementation targets the [Jsonnet Spec](https://jsonnet.org/ref/spec.html) and uses the [test suite](https://github.com/google/jsonnet/tree/master/test_suite) published in the original Jsonnet repository at version 0.22.0. This test suite is downloaded at build time and all the tests pass. Because this hasn't been used in professional environment, it is suspected that there are bugs in both the implementation of the spec and the standard library.

The conformance suite is downloaded directly from upstream Jsonnet repository at build time to ensure the tests are exactly what ships with the original Jsonnet.

### CLI Feature Support

| Feature                       | Status        | Notes                                                         |
|-------------------------------|---------------|---------------------------------------------------------------|
| `--ext-str`                   | Supported     | Both `--ext-str key=val` and `--ext-str=key=val` forms        |
| `--ext-code`                  | Partial       | JSON values only; arbitrary Jsonnet expressions not supported |
| `--tla-str` / `--tla-code`    | Not supported | Top-level arguments are not implemented                       |
| `--max-stack`                 | Not supported | Stack size is hardcoded at 65536                              |
| `--multi` (multi-file output) | Not supported | Single stdout output only                                     |
| `-J` / `--jpath`              | Supported     | Both `-J path` and `-Jpath` forms                             |

## Unit Test runner
The jsonnet test runner allows a developer to run a test_file.jsonnet and provide lcov output for all the imported files used in each of the test functions. This gives developers a tool to test their complex configurations and prevent bugs.

To use it directly from Bazel:

```bash
# get the usage
bazel run //:jsonnet_test_runner
Usage: jsonnet_test_runner [--coverage] [--lcov-output <path>] [--suite-name <name>] [--test-name <filter>] [-J <path>]... <test_file.jsonnet>
```

This can be used to run a test_file and provide lcov output for all the imported files used in the test. A test file looks like this:

```jsonnet
local root = import "end2end/import_integration_test.libsonnet";

{
  testBasicEquality(): std.assertEqual(root.rootValue + root.rootValue, 2),
    testStringOps(): std.assertEqual(std.length(root.stringValue()), 5),
  testAssertKeyword():
    assert std.type(root.stringValue()) == "string" : "type check";
    true,
  testArrayLength(): std.assertEqual(std.length(root.arrayValue), 3),
  // Will generate a Skip message
  skip_testWillFail(): std.assertEqual(std.length(root.arrayValue), 2),
}
```
Each `test` prefixed function in the returned object is run independently and when that function produces a `RuntimeError` then the test is marked as failed. If the function produces a `CompilerError` or a `ScanError` then the test is marked as exception. Test coverage is calculated and merged across each test run executed to produce a lcov file per jsonnet_test_runner invocation.

### Using Unit Test with Bazel
To use this with the bazel rules, add the following:

``` starlark
jsonnet_test(
    name = "test_framework_integration_test",
    src = "test_framework_test.jsonnet",
    coverage = "lcov",
    deps = [
        ":integration_lib",
    ],
)
```
and run `bazel coverage` command to generate a lcov file per bazel target. Each `deps` must be a `jsonnet_library`, and `data` set can included the `.json` outputs from jsonnet_to_json targets.


## Experimental: Ahead of time compilation
The project provides a `jsonnet_compiler` binary (`bazel build //:jsonnet_compiler`) which produces a [Apache Fory](https://fory.apache.org/) serialized file suffixed with `c` -- e.g. `.libsonnetc`. When the virtual machine is told to `local x = import "a_jsonnet_lib.libsonnet"`, the virtual machine *first* looks for a `a_jsonnet_lib.libsonnetc` file and if it exists loads that and skips parsing and compiling. This could provide performance benefits, those benefits are not yet objectively measured.
