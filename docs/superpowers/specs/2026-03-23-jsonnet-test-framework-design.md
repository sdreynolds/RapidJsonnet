# Jsonnet Test Framework Design

## Overview

A test framework for RapidJsonnet that provides a dedicated test runner binary, pluggable reporting, and span-level coverage collection. Tests are written as Jsonnet library objects where functions prefixed with `test` are automatically discovered and executed.

## Test Authoring Model

Test files are standard Jsonnet files that export a top-level object. Any field whose name starts with `test` is treated as a test case. Other fields are ignored (available as helpers).

```jsonnet
{
  testAddition():
    std.assertEqual(1 + 1, 2),

  testAssertKeyword():
    assert 1 + 1 == 2 : "math is broken";
    true,

  helperFunc(x): x * 2,  // not a test, ignored
}
```

**Pass/fail semantics:**
- A test **passes** if the function returns without throwing a runtime error.
- A test **fails** if a runtime error is thrown (via `assert`, `std.assertEqual`, `error`, etc.).
- The error message is captured and included in the test report.

## Architecture

### Coverage Collector

A new `CoverageCollector` struct tracks which source spans are executed:

```rust
pub struct CoverageCollector {
    hit_spans: HashSet<(String, Range<usize>)>,  // (source_id, span range)
}
```

Lives in `src/coverage.rs`. Provides:
- `record(source_id, span)` — insert a span hit
- `merge(other)` — combine coverage from multiple test runs
- Accessors for reporting

### VM Integration

The `VirtualMachine` gains an optional `CoverageCollector` field:

```rust
coverage_collector: Option<CoverageCollector>
```

In the fetch-execute loop, after `instruction_start_ip` is set (already happens for error reporting), the VM conditionally records the span:

```rust
if let Some(ref mut collector) = self.coverage_collector {
    let span = current_chunk.get_span(self.instruction_start_ip);
    collector.record(source_id, span);
}
```

New VM methods:
- `enable_coverage()` — set the collector
- `take_coverage() -> Option<CoverageCollector>` — extract collected data

When coverage is disabled (`None`), cost is a single branch prediction per instruction.

### Test Runner

The core test execution logic lives in `src/test_runner.rs`:

```rust
pub struct TestResult {
    pub name: String,
    pub outcome: TestOutcome,
    pub coverage: CoverageCollector,
}

pub enum TestOutcome {
    Pass,
    Fail { message: String },
}
```

**Execution flow:**
1. Compile and evaluate the test source file to obtain a top-level object.
2. Iterate the object's visible fields, filtering those whose name starts with `"test"`.
3. For each test function:
   - Enable coverage on the VM.
   - Call the function with no arguments.
   - If it returns without error: mark as pass.
   - If it throws a runtime error: mark as fail, capture the error message.
   - Extract coverage data from the VM.
4. Pass `Vec<TestResult>` and aggregated coverage to the reporter.

### Reporter Trait

Defined in `src/test_reporter.rs`:

```rust
pub trait TestReporter {
    fn on_test_start(&mut self, name: &str);
    fn on_test_complete(&mut self, result: &TestResult);
    fn on_suite_complete(&mut self, results: &[TestResult]);
}
```

**Initial implementation — `TextReporter`:**

Writes to a `Write` sink. Output format:

```
Running 3 tests...

PASS  testAddition
PASS  testStringConcat
FAIL  testBadMath
      assertion failed: expected 3, got 4

3 tests: 2 passed, 1 failed
```

Future reporters (TAP, JUnit XML, coverage report) implement the same trait. The runner selects the reporter based on a CLI flag (`--reporter text`), defaulting to `TextReporter`.

### Test Runner Binary

`src/jsonnet_test_runner.rs` — the binary entry point:

1. Parse CLI args: test source file, `-J` import paths, `--reporter` flag.
2. Instantiate the VM and reporter.
3. Invoke the test runner logic.
4. Exit code 0 if all tests pass, non-zero if any fail.

### Bazel Rule

`jsonnet_test` rule added to `rules/jsonnet.bzl`:

```python
jsonnet_test(
    name = "my_lib_test",
    src = "my_lib_test.jsonnet",
    deps = [":my_lib"],
    data = [],
)
```

Implementation:
- Wraps the `jsonnet_test_runner` binary as a test executable.
- Collects transitive sources from `deps` via `JsonnetLibraryInfo` (same pattern as `jsonnet_to_json`).
- Constructs args: test source file path + `-J` paths for import resolution.
- The runner binary is a tool dependency.
- Produces a `native_test` or equivalent so `bazel test` works natively.

## File Organization

**New files:**
| File | Purpose |
|------|---------|
| `src/coverage.rs` | `CoverageCollector` struct |
| `src/test_runner.rs` | Test discovery, execution loop, `TestResult`/`TestOutcome` |
| `src/test_reporter.rs` | `TestReporter` trait, `TextReporter` |
| `src/jsonnet_test_runner.rs` | Binary entry point |

**Modified files:**
| File | Change |
|------|--------|
| `src/virtual_machine.rs` | Add optional `CoverageCollector`, record spans in execution loop |
| `BUILD.bazel` | New library/binary/test targets |
| `rules/jsonnet.bzl` | Add `jsonnet_test` rule |

**Unchanged:** `scanner.rs`, `parser.rs`, `compiler.rs`, `chunk.rs` — existing span infrastructure is sufficient.

## Design Decisions

1. **VM-integrated coverage** over callback/observer pattern — simpler, minimal overhead, sufficient for current needs. Can refactor to observer pattern later if needed.
2. **Separate binary** over subcommand — clean separation of concerns, natural fit for Bazel tool dependency.
3. **Span-level coverage** over line-level — leverages existing run-length encoded span tracking in chunks, provides expression-level granularity.
4. **Error-based test failure** over return-value checking — works with existing `assert`, `std.assertEqual`, and `error` mechanisms without new language features.
5. **Reporter trait** over hardcoded output — minimal abstraction cost, enables future TAP/JUnit/coverage reporters without changing the runner.
