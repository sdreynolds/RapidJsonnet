# Jsonnet Test Framework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a test runner binary, pluggable reporter, and span-level coverage collector that discovers and runs `test*` functions from Jsonnet objects, with a matching `jsonnet_test` Bazel rule.

**Architecture:** A `CoverageCollector` struct records hit spans during VM execution via an opt-in field. A test runner compiles a Jsonnet file, iterates `test*` fields on the result object, calls each as a zero-arg closure, and reports pass/fail through a `TestReporter` trait. A `jsonnet_test` Bazel rule wraps the binary as a native test.

**Tech Stack:** Rust, Bazel (rules_rust, rules_shell), existing RapidJsonnet VM/compiler/memory_manager infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-23-jsonnet-test-framework-design.md`

---

### Task 1: CoverageCollector struct

**Files:**
- Create: `src/coverage.rs`
- Create: `BUILD.bazel` (add `coverage` rust_library target)

- [ ] **Step 1: Write the failing test**

In `src/coverage.rs`, add the module with tests inline:

```rust
use std::collections::{HashMap, HashSet};
use std::ops::Range;

/// Collects span-level coverage data during VM execution.
/// Keyed by source_id, values are sets of (start, end) span pairs.
pub struct CoverageCollector {
    hit_spans: HashMap<String, HashSet<(usize, usize)>>,
}

impl CoverageCollector {
    pub fn new() -> Self {
        Self {
            hit_spans: HashMap::new(),
        }
    }

    /// Record a span hit for a given source file.
    pub fn record(&mut self, source_id: &str, span: &Range<usize>) {
        self.hit_spans
            .entry(source_id.to_string())
            .or_default()
            .insert((span.start, span.end));
    }

    /// Merge another collector's data into this one.
    pub fn merge(&mut self, other: CoverageCollector) {
        for (source_id, spans) in other.hit_spans {
            self.hit_spans
                .entry(source_id)
                .or_default()
                .extend(spans);
        }
    }

    /// Get all source IDs that have coverage data.
    pub fn source_ids(&self) -> Vec<&str> {
        self.hit_spans.keys().map(|s| s.as_str()).collect()
    }

    /// Get the set of hit spans for a given source file.
    pub fn spans_for_source(&self, source_id: &str) -> Option<&HashSet<(usize, usize)>> {
        self.hit_spans.get(source_id)
    }

    /// Total number of unique spans hit across all sources.
    pub fn total_spans_hit(&self) -> usize {
        self.hit_spans.values().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_retrieve() {
        let mut collector = CoverageCollector::new();
        collector.record("test.jsonnet", &(0..5));
        collector.record("test.jsonnet", &(10..15));
        collector.record("other.jsonnet", &(0..3));

        assert_eq!(collector.source_ids().len(), 2);
        assert_eq!(collector.spans_for_source("test.jsonnet").unwrap().len(), 2);
        assert_eq!(collector.spans_for_source("other.jsonnet").unwrap().len(), 1);
        assert_eq!(collector.total_spans_hit(), 3);
    }

    #[test]
    fn test_deduplication() {
        let mut collector = CoverageCollector::new();
        collector.record("test.jsonnet", &(0..5));
        collector.record("test.jsonnet", &(0..5)); // duplicate
        assert_eq!(collector.total_spans_hit(), 1);
    }

    #[test]
    fn test_merge() {
        let mut a = CoverageCollector::new();
        a.record("test.jsonnet", &(0..5));

        let mut b = CoverageCollector::new();
        b.record("test.jsonnet", &(10..15));
        b.record("other.jsonnet", &(0..3));

        a.merge(b);
        assert_eq!(a.source_ids().len(), 2);
        assert_eq!(a.spans_for_source("test.jsonnet").unwrap().len(), 2);
        assert_eq!(a.total_spans_hit(), 3);
    }

    #[test]
    fn test_empty_collector() {
        let collector = CoverageCollector::new();
        assert_eq!(collector.total_spans_hit(), 0);
        assert!(collector.spans_for_source("nope").is_none());
    }
}
```

- [ ] **Step 2: Add BUILD.bazel target**

Add to `BUILD.bazel` after the `serialized_chunk` library target:

```python
rust_library(
    name = "coverage",
    srcs = ["src/coverage.rs"],
)
```

And add a test target after the existing test targets:

```python
rust_test(
    name = "coverage_test",
    crate = ":coverage"
)
```

And add `:coverage` to the `rustfmt_test` targets list.

- [ ] **Step 3: Run tests to verify they pass**

Run: `bazel test //:coverage_test`
Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/coverage.rs
git commit -m "feat: add CoverageCollector for span-level coverage tracking"
```

---

### Task 2: VM coverage integration

**Files:**
- Modify: `src/virtual_machine.rs`
- Modify: `BUILD.bazel` (add `:coverage` dep to `virtual_machine`)

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/virtual_machine.rs` `mod tests` block:

```rust
#[test]
fn test_coverage_collector_records_spans() {
    let source = "1 + 2";
    let mut scanner = scanner::Scanner::new(source, "test_coverage");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test_coverage");
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    vm.enable_coverage();
    let _ = vm.interpret();
    let collector = vm.take_coverage().unwrap();

    assert!(collector.total_spans_hit() > 0);
    assert!(collector.spans_for_source("test_coverage").is_some());
}

#[test]
fn test_coverage_disabled_by_default() {
    let source = "1 + 2";
    let mut scanner = scanner::Scanner::new(source, "test_no_cov");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test_no_cov");
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let _ = vm.interpret();
    assert!(vm.take_coverage().is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `bazel test //:virtual_machine_test --test_filter="test_coverage"`
Expected: FAIL — `enable_coverage` and `take_coverage` don't exist yet.

- [ ] **Step 3: Add coverage dependency to BUILD.bazel**

Add `:coverage` to the `virtual_machine` rust_library deps list in `BUILD.bazel`.

- [ ] **Step 4: Add coverage field and methods to VirtualMachine**

In `src/virtual_machine.rs`:

1. Add import at the top: `use coverage::CoverageCollector;`

2. Add field to `VirtualMachine` struct (after `jpaths`):
```rust
    /// Optional coverage collector for span tracking during test runs
    coverage_collector: Option<CoverageCollector>,
```

3. Initialize in both `new` and `new_from_owned` constructors:
```rust
    coverage_collector: None,
```

4. Add public methods (after `set_jpaths`):
```rust
    /// Enable span-level coverage collection.
    pub fn enable_coverage(&mut self) {
        self.coverage_collector = Some(CoverageCollector::new());
    }

    /// Extract collected coverage data, leaving None in its place.
    pub fn take_coverage(&mut self) -> Option<CoverageCollector> {
        self.coverage_collector.take()
    }
```

5. In the `interpret_until` method, right after `self.instruction_start_ip = self.current_frame().ip;` (line ~1408), add coverage recording:
```rust
    // Record span for coverage if enabled
    if let Some(ref mut collector) = self.coverage_collector {
        let chunk = self.current_chunk();
        if let Some(span) = chunk.get_span(self.instruction_start_ip) {
            collector.record(&chunk.source_id, span);
        }
    }
```

Note: This must be placed carefully — `self.current_chunk()` borrows `self`, but we need `&mut self.coverage_collector`. Since `coverage_collector` is a separate field from `frames`/`memory_manager`, the borrow checker should allow this if we get the span data first, then record:

```rust
    if self.coverage_collector.is_some() {
        let chunk = self.current_chunk();
        let span = chunk.get_span(self.instruction_start_ip).cloned();
        let source_id = chunk.source_id.to_string();
        if let (Some(ref mut collector), Some(span)) = (&mut self.coverage_collector, span) {
            collector.record(&source_id, &span);
        }
    }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `bazel test //:virtual_machine_test --test_filter="test_coverage"`
Expected: Both tests pass.

- [ ] **Step 6: Run full test suite to verify no regressions**

Run: `bazel test //...`
Expected: All existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add src/virtual_machine.rs BUILD.bazel
git commit -m "feat: add optional coverage collection to VM execution loop"
```

---

### Task 3: Public VM methods for test runner

**Files:**
- Modify: `src/virtual_machine.rs`

The test runner needs two operations after `interpret()` returns:
1. **Force a field thunk** — object field values are thunks (closures with arity=2 taking self/super). Forcing yields the actual value.
2. **Call a zero-arg closure** — the forced value of a `testFoo(): expr` field is a zero-arg closure that must be called to run the test.

The existing private `execute_thunk_sync` and `call_closure`/`interpret_until` already implement both patterns. We expose public wrappers.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/virtual_machine.rs`:

```rust
#[test]
fn test_force_field_thunk() {
    // Evaluate an object, then force a field thunk to get its value
    let source = r#"{ x: 42 }"#;
    let mut scanner = scanner::Scanner::new(source, "test_force");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test_force");
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret().unwrap();

    if let Value::Object(obj_idx) = result {
        let obj = vm.memory_manager().load_object(obj_idx);
        let (thunk_ci, super_obj) = {
            let field = obj.properties.values().next().unwrap();
            match field.value {
                Value::Closure(ci) => (ci, field.super_obj),
                _ => panic!("Expected thunk closure"),
            }
        };
        let val = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
        if let Value::Number(n) = val {
            assert_eq!(n, 42.0);
        } else {
            panic!("Expected number, got {:?}", val);
        }
    } else {
        panic!("Expected object");
    }
}

#[test]
fn test_call_test_closure_pass() {
    // Evaluate test object, force thunk to get the function, then call it
    let source = r#"{ testPass(): std.assertEqual(1 + 1, 2) }"#;
    let mut scanner = scanner::Scanner::new(source, "test_call");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test_call");
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret().unwrap();

    if let Value::Object(obj_idx) = result {
        let obj = vm.memory_manager().load_object(obj_idx);
        let (thunk_ci, super_obj) = {
            let mut found = None;
            for (key, field) in &obj.properties {
                let name = vm.memory_manager().load_string(*key);
                if name == "testPass" {
                    if let Value::Closure(ci) = field.value {
                        found = Some((ci, field.super_obj));
                    }
                }
            }
            found.expect("testPass should be a thunk")
        };
        // Force the thunk to get the zero-arg test function
        let forced = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
        if let Value::Closure(func_ci) = forced {
            let result = vm.call_test_closure(func_ci);
            assert!(result.is_ok(), "testPass should succeed");
        } else {
            panic!("Expected closure after forcing thunk");
        }
    } else {
        panic!("Expected object result");
    }
}

#[test]
fn test_call_test_closure_fail() {
    let source = r#"{ testFail(): std.assertEqual(1, 2) }"#;
    let mut scanner = scanner::Scanner::new(source, "test_call_fail");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test_call_fail");
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret().unwrap();

    if let Value::Object(obj_idx) = result {
        let obj = vm.memory_manager().load_object(obj_idx);
        let (thunk_ci, super_obj) = {
            let mut found = None;
            for (key, field) in &obj.properties {
                let name = vm.memory_manager().load_string(*key);
                if name == "testFail" {
                    if let Value::Closure(ci) = field.value {
                        found = Some((ci, field.super_obj));
                    }
                }
            }
            found.expect("testFail should be a thunk")
        };
        let forced = vm.force_field_thunk(thunk_ci, obj_idx, super_obj).unwrap();
        if let Value::Closure(func_ci) = forced {
            let result = vm.call_test_closure(func_ci);
            assert!(result.is_err(), "testFail should return an error");
        } else {
            panic!("Expected closure after forcing thunk");
        }
    } else {
        panic!("Expected object result");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `bazel test //:virtual_machine_test --test_filter="test_call_test_closure|test_force_field_thunk"`
Expected: FAIL — `force_field_thunk`, `call_test_closure`, `memory_manager()` don't exist.

- [ ] **Step 3: Implement the public methods**

Add public methods to `VirtualMachine` (near `interpret`):

```rust
    /// Get a reference to the memory manager (for test runner field inspection).
    pub fn memory_manager(&self) -> &MemoryManager {
        &self.memory_manager
    }

    /// Force a field thunk to get its actual value.
    /// Object field values are closures with arity=2 (self, super) that must be
    /// forced to obtain the real value. This mirrors the ObjectIndex opcode behavior.
    pub fn force_field_thunk(
        &mut self,
        closure_index: ClosureIndex,
        obj_index: ObjectIndex,
        super_obj: Option<ObjectIndex>,
    ) -> Result<Value, RuntimeError> {
        self.execute_thunk_sync(closure_index, Some(obj_index), super_obj)
    }

    /// Call a zero-argument closure and run it to completion.
    /// Used by the test runner to invoke individual test functions
    /// after force_field_thunk has yielded the function closure.
    pub fn call_test_closure(
        &mut self,
        closure_index: ClosureIndex,
    ) -> Result<Value, RuntimeError> {
        self.push(Value::Closure(closure_index))?;
        let target_frame_count = self.frame_count;
        self.call_closure(closure_index, 0, None, None)?;
        self.interpret_until(target_frame_count)
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `bazel test //:virtual_machine_test --test_filter="test_call_test_closure|test_force_field_thunk"`
Expected: All 3 tests pass.

- [ ] **Step 5: Run full test suite**

Run: `bazel test //...`
Expected: No regressions.

- [ ] **Step 6: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "feat: add force_field_thunk and call_test_closure public APIs"
```

---

### Task 4: TestReporter trait and TextReporter

**Files:**
- Create: `src/test_reporter.rs`
- Modify: `BUILD.bazel` (add `test_reporter` target)

- [ ] **Step 1: Write the test_reporter module**

Create `src/test_reporter.rs`:

```rust
use std::io::Write;

/// Result of a single test execution.
pub struct TestResult {
    pub name: String,
    pub outcome: TestOutcome,
}

/// Whether a test passed or failed.
pub enum TestOutcome {
    Pass,
    Fail { message: String },
}

/// Trait for pluggable test output formats.
pub trait TestReporter {
    fn on_test_start(&mut self, name: &str);
    fn on_test_complete(&mut self, result: &TestResult);
    fn on_suite_complete(&mut self, results: &[TestResult]);
}

/// Basic text reporter that writes human-readable output.
pub struct TextReporter<W: Write> {
    writer: W,
}

impl<W: Write> TextReporter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W: Write> TestReporter for TextReporter<W> {
    fn on_test_start(&mut self, _name: &str) {
        // TextReporter prints results on completion, not on start
    }

    fn on_test_complete(&mut self, result: &TestResult) {
        match &result.outcome {
            TestOutcome::Pass => {
                writeln!(self.writer, "PASS  {}", result.name).ok();
            }
            TestOutcome::Fail { message } => {
                writeln!(self.writer, "FAIL  {}", result.name).ok();
                writeln!(self.writer, "      {}", message).ok();
            }
        }
    }

    fn on_suite_complete(&mut self, results: &[TestResult]) {
        let total = results.len();
        let passed = results
            .iter()
            .filter(|r| matches!(r.outcome, TestOutcome::Pass))
            .count();
        let failed = total - passed;

        writeln!(self.writer).ok();
        writeln!(
            self.writer,
            "{} tests: {} passed, {} failed",
            total, passed, failed
        )
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_reporter_all_pass() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results = vec![
            TestResult {
                name: "testA".to_string(),
                outcome: TestOutcome::Pass,
            },
            TestResult {
                name: "testB".to_string(),
                outcome: TestOutcome::Pass,
            },
        ];

        for r in &results {
            reporter.on_test_complete(r);
        }
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("PASS  testA"));
        assert!(output_str.contains("PASS  testB"));
        assert!(output_str.contains("2 tests: 2 passed, 0 failed"));
    }

    #[test]
    fn test_text_reporter_with_failure() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results = vec![
            TestResult {
                name: "testGood".to_string(),
                outcome: TestOutcome::Pass,
            },
            TestResult {
                name: "testBad".to_string(),
                outcome: TestOutcome::Fail {
                    message: "expected 3, got 4".to_string(),
                },
            },
        ];

        for r in &results {
            reporter.on_test_complete(r);
        }
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("PASS  testGood"));
        assert!(output_str.contains("FAIL  testBad"));
        assert!(output_str.contains("expected 3, got 4"));
        assert!(output_str.contains("2 tests: 1 passed, 1 failed"));
    }

    #[test]
    fn test_text_reporter_empty_suite() {
        let mut output = Vec::new();
        let mut reporter = TextReporter::new(&mut output);

        let results: Vec<TestResult> = vec![];
        reporter.on_suite_complete(&results);

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("0 tests: 0 passed, 0 failed"));
    }
}
```

- [ ] **Step 2: Add BUILD.bazel target**

Add to `BUILD.bazel`:

```python
rust_library(
    name = "test_reporter",
    srcs = ["src/test_reporter.rs"],
)
```

And test target:

```python
rust_test(
    name = "test_reporter_test",
    crate = ":test_reporter"
)
```

And add `:test_reporter` to the `rustfmt_test` targets list.

- [ ] **Step 3: Run tests**

Run: `bazel test //:test_reporter_test`
Expected: All 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/test_reporter.rs BUILD.bazel
git commit -m "feat: add TestReporter trait and TextReporter implementation"
```

---

### Task 5: Test runner library

**Files:**
- Create: `src/test_runner.rs`
- Modify: `BUILD.bazel` (add `test_runner` target)

- [ ] **Step 1: Write the test runner module**

Create `src/test_runner.rs`:

```rust
use chunk::{ClosureIndex, FieldVisibility, ObjectIndex, RuntimeError, StringIndex, Value};
use coverage::CoverageCollector;
use memory_manager::MemoryManager;
use test_reporter::{TestOutcome, TestReporter, TestResult};
use virtual_machine::VirtualMachine;

/// Error during test discovery or setup (not a test failure).
#[derive(Debug)]
pub enum TestRunnerError {
    /// The top-level value was not an object.
    NotAnObject,
    /// A test* field was not a zero-argument function after thunk forcing.
    NotAFunction { name: String },
    /// A runtime error during thunk forcing or setup.
    RuntimeError(RuntimeError),
    /// Compilation or evaluation failed before tests could run.
    SetupError(String),
}

impl std::fmt::Display for TestRunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestRunnerError::NotAnObject => {
                write!(f, "Test file must evaluate to an object")
            }
            TestRunnerError::NotAFunction { name } => {
                write!(f, "Test field '{}' is not a zero-argument function", name)
            }
            TestRunnerError::RuntimeError(e) => {
                write!(f, "Runtime error: {}", e.message)
            }
            TestRunnerError::SetupError(msg) => {
                write!(f, "Setup error: {}", msg)
            }
        }
    }
}

/// A discovered test field (thunk not yet forced).
struct DiscoveredField {
    name: String,
    thunk_closure: ClosureIndex,
    super_obj: Option<ObjectIndex>,
}

/// Discover test* fields from a top-level object.
/// Returns fields sorted alphabetically. Thunks are NOT forced yet.
fn discover_test_fields(
    value: &Value,
    memory_manager: &MemoryManager,
) -> Result<(ObjectIndex, Vec<DiscoveredField>), TestRunnerError> {
    let obj_idx = match value {
        Value::Object(idx) => *idx,
        _ => return Err(TestRunnerError::NotAnObject),
    };

    let obj = memory_manager.load_object(obj_idx);
    let mut fields = Vec::new();

    for (key_idx, field) in &obj.properties {
        // Skip hidden fields
        if matches!(field.visibility, FieldVisibility::Hidden) {
            continue;
        }

        let name = memory_manager.load_string(*key_idx).to_string();
        if !name.starts_with("test") {
            continue;
        }

        // All object field values should be thunks (Closures with arity=2)
        match field.value {
            Value::Closure(ci) => {
                fields.push(DiscoveredField {
                    name,
                    thunk_closure: ci,
                    super_obj: field.super_obj,
                });
            }
            _ => {
                // Raw values are not expected for function fields
                return Err(TestRunnerError::NotAFunction { name });
            }
        }
    }

    fields.sort_by(|a, b| a.name.cmp(&b.name));
    Ok((obj_idx, fields))
}

/// Run all discovered tests, reporting results through the reporter.
/// Returns true if all tests passed, false if any failed.
///
/// For each test* field:
/// 1. Force the field thunk (arity=2, self/super) to get the actual value.
/// 2. Verify the forced value is a zero-arg closure.
/// 3. Call the closure to run the test.
pub fn run_tests(
    vm: &mut VirtualMachine,
    top_level_value: &Value,
    reporter: &mut dyn TestReporter,
    collect_coverage: bool,
) -> Result<bool, TestRunnerError> {
    let (obj_idx, fields) = discover_test_fields(top_level_value, vm.memory_manager())?;

    let mut results = Vec::with_capacity(fields.len());

    for field in &fields {
        reporter.on_test_start(&field.name);

        if collect_coverage {
            vm.enable_coverage();
        }

        // Step 1: Force the thunk to get the actual value
        let forced_value = vm
            .force_field_thunk(field.thunk_closure, obj_idx, field.super_obj)
            .map_err(TestRunnerError::RuntimeError)?;

        // Step 2: Verify it's a zero-arg function closure
        let func_closure = match forced_value {
            Value::Closure(ci) => {
                let closure = vm.memory_manager().load_closure(ci);
                let func = vm.memory_manager().load_function(closure.function);
                if func.required_params > 0 {
                    return Err(TestRunnerError::NotAFunction {
                        name: field.name.clone(),
                    });
                }
                ci
            }
            _ => {
                return Err(TestRunnerError::NotAFunction {
                    name: field.name.clone(),
                });
            }
        };

        // Step 3: Call the test function
        let outcome = match vm.call_test_closure(func_closure) {
            Ok(_) => TestOutcome::Pass,
            Err(e) => TestOutcome::Fail { message: e.message },
        };

        let result = TestResult {
            name: field.name.clone(),
            outcome,
        };

        reporter.on_test_complete(&result);
        results.push(result);
    }

    reporter.on_suite_complete(&results);

    let all_passed = results
        .iter()
        .all(|r| matches!(r.outcome, TestOutcome::Pass));
    Ok(all_passed)
}
```

- [ ] **Step 2: Add BUILD.bazel target**

Add to `BUILD.bazel`:

```python
rust_library(
    name = "test_runner",
    srcs = ["src/test_runner.rs"],
    deps = [
        ":chunk",
        ":coverage",
        ":memory_manager",
        ":test_reporter",
        ":virtual_machine",
    ],
)
```

And add `:test_runner` to the `rustfmt_test` targets list.

- [ ] **Step 3: Run build to verify it compiles**

Run: `bazel build //:test_runner`
Expected: Build succeeds. (Integration testing happens in Task 6.)

- [ ] **Step 4: Commit**

```bash
git add src/test_runner.rs BUILD.bazel
git commit -m "feat: add test runner library with discovery and execution"
```

---

### Task 6: Test runner binary

**Files:**
- Create: `src/jsonnet_test_runner.rs`
- Modify: `BUILD.bazel` (add binary target)

- [ ] **Step 1: Write the binary**

Create `src/jsonnet_test_runner.rs`:

```rust
use compiler::Compiler;
use memory_manager::MemoryManager;
use scanner::Scanner;
use test_reporter::TextReporter;
use test_runner;
use virtual_machine::VirtualMachine;
use std::env;
use std::fs;
use std::io;
use std::process;

fn main() {
    let mut jpaths: Vec<String> = Vec::new();
    let mut collect_coverage = false;
    let mut args_iter = env::args().skip(1).peekable();

    while let Some(arg) = args_iter.peek().cloned() {
        if arg == "-J" || arg == "--jpath" {
            args_iter.next();
            if let Some(path) = args_iter.next() {
                jpaths.push(path);
            }
        } else if arg.starts_with("-J") && arg.len() > 2 {
            jpaths.push(arg[2..].to_string());
            args_iter.next();
        } else if arg.starts_with("--jpath=") {
            jpaths.push(arg.trim_start_matches("--jpath=").to_string());
            args_iter.next();
        } else if arg == "--coverage" {
            collect_coverage = true;
            args_iter.next();
        } else {
            break;
        }
    }

    let filename = match args_iter.next() {
        Some(f) => f,
        None => {
            eprintln!("Usage: jsonnet_test_runner [--coverage] [-J <path>]... <test_file.jsonnet>");
            process::exit(2);
        }
    };

    let content = match fs::read_to_string(&filename) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error reading {}: {}", filename, e);
            process::exit(2);
        }
    };

    let mut scanner = Scanner::new(&content, &filename);
    let mut memory_manager = MemoryManager::new();
    let compiler = Compiler::new(&mut scanner, &filename);

    let chunk = match compiler.compile(&mut memory_manager) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Compilation failed: {}", e.message);
            process::exit(2);
        }
    };

    let mut vm = VirtualMachine::new(chunk, memory_manager);
    vm.set_jpaths(jpaths);

    let top_level = match vm.interpret() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error evaluating test file: {}", e.message);
            process::exit(2);
        }
    };

    let stdout = io::stdout();
    let mut reporter = TextReporter::new(stdout.lock());

    match test_runner::run_tests(&mut vm, &top_level, &mut reporter, collect_coverage) {
        Ok(true) => process::exit(0),
        Ok(false) => process::exit(1),
        Err(e) => {
            eprintln!("Test runner error: {}", e);
            process::exit(2);
        }
    }
}
```

- [ ] **Step 2: Add BUILD.bazel targets**

Add to `BUILD.bazel`:

```python
rust_binary(
    name = "jsonnet_test_runner",
    srcs = ["src/jsonnet_test_runner.rs"],
    deps = [
        ":compiler",
        ":memory_manager",
        ":scanner",
        ":test_reporter",
        ":test_runner",
        ":virtual_machine",
    ],
    crate_features = [
        "stress_gc"
    ]
)
```

And add `:jsonnet_test_runner` to the `rustfmt_test` targets list.

- [ ] **Step 3: Build the binary**

Run: `bazel build //:jsonnet_test_runner`
Expected: Build succeeds.

- [ ] **Step 4: Create a sample test file and run manually**

Create `end2end/sample_test.jsonnet`:

```jsonnet
{
  testAddition(): std.assertEqual(1 + 1, 2),
  testStringConcat(): std.assertEqual("hello" + " " + "world", "hello world"),
  testAssertKeyword():
    assert 1 == 1 : "basic equality";
    true,
  testFailure(): std.assertEqual(1, 2),
}
```

Run manually:
```bash
bazel run //:jsonnet_test_runner -- $(pwd)/end2end/sample_test.jsonnet
```

Expected output (test order is alphabetical):
```
PASS  testAddition
PASS  testAssertKeyword
FAIL  testFailure
      ...assertion error message...
PASS  testStringConcat

4 tests: 3 passed, 1 failed
```

Expected exit code: 1 (because testFailure fails).

- [ ] **Step 5: Verify passing test file exits 0**

Create `end2end/passing_test.jsonnet`:

```jsonnet
{
  testOne(): std.assertEqual(1, 1),
  testTwo(): std.assertEqual("a", "a"),
}
```

Run:
```bash
bazel run //:jsonnet_test_runner -- $(pwd)/end2end/passing_test.jsonnet
echo $?
```

Expected: exit code 0.

- [ ] **Step 6: Clean up sample test files and commit**

Remove `end2end/sample_test.jsonnet` and `end2end/passing_test.jsonnet` (they were for manual verification only).

```bash
git add src/jsonnet_test_runner.rs BUILD.bazel
git commit -m "feat: add jsonnet_test_runner binary"
```

---

### Task 7: `jsonnet_test` Bazel rule

**Files:**
- Modify: `rules/jsonnet.bzl`
- Create: `end2end/test_framework_test.jsonnet` (integration test)
- Modify: `end2end/BUILD.bazel` (add jsonnet_test usage)

- [ ] **Step 1: Add `jsonnet_test` rule to `rules/jsonnet.bzl`**

Add the rule implementation after `jsonnet_to_json`:

```python
def _jsonnet_test_impl(ctx):
    src_file = ctx.file.src

    # Collect transitive inputs from deps
    transitive_srcs_deps = []
    transitive_data_deps = []

    dep_srcs, dep_data = _collect_transitive(ctx.attr.deps)
    transitive_srcs_deps.extend(dep_srcs)
    transitive_data_deps.extend(dep_data)

    all_srcs = depset([src_file], transitive = transitive_srcs_deps)
    all_data = depset(ctx.files.data, transitive = transitive_data_deps)
    all_inputs = depset(transitive = [all_srcs, all_data])

    # Build the command: runner -J <dir> <src>
    # Use the source file's root dir for -J so imports resolve
    args = ["-J", "."]
    args.append(src_file.path)

    # Create a wrapper script that invokes the runner
    wrapper = ctx.actions.declare_file(ctx.label.name + "_runner.sh")
    ctx.actions.write(
        output = wrapper,
        content = "{tool} {args}".format(
            tool = _q(ctx.executable._runner.short_path),
            args = " ".join([_q(a) for a in args]),
        ),
        is_executable = True,
    )

    runfiles = ctx.runfiles(
        files = [ctx.executable._runner],
        transitive_files = all_inputs,
    )
    runfiles = runfiles.merge(ctx.attr._runner[DefaultInfo].default_runfiles)

    return [
        DefaultInfo(
            executable = wrapper,
            runfiles = runfiles,
        ),
    ]

jsonnet_test = rule(
    implementation = _jsonnet_test_impl,
    test = True,
    attrs = {
        "src": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "The Jsonnet test source file.",
        ),
        "deps": attr.label_list(
            providers = [JsonnetLibraryInfo],
            doc = "jsonnet_library dependencies.",
        ),
        "data": attr.label_list(
            allow_files = True,
            doc = "Data files available at runtime.",
        ),
        "_runner": attr.label(
            default = Label("//:jsonnet_test_runner"),
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Runs a Jsonnet test file using the test runner. Functions prefixed with 'test' are discovered and executed.",
)
```

- [ ] **Step 2: Create an integration test file**

Create `end2end/test_framework_test.jsonnet`:

```jsonnet
{
  testBasicEquality(): std.assertEqual(1 + 1, 2),
  testStringOps(): std.assertEqual(std.length("hello"), 5),
  testAssertKeyword():
    assert std.type("hello") == "string" : "type check";
    true,
  testArrayLength(): std.assertEqual(std.length([1, 2, 3]), 3),
}
```

- [ ] **Step 3: Update end2end/BUILD.bazel**

First, exclude test framework files from the existing glob so they don't get picked up by the `sh_test` comprehension (which runs files through the regular main binary):

Change line 3 from:
```python
JSONNET_FILES = glob(["*.jsonnet"])
```
to:
```python
JSONNET_FILES = glob(["*.jsonnet"], exclude = ["*_framework_test.jsonnet", "test_runner_*.jsonnet"])
```

Then add at the top, extending the existing load:
```python
load("//rules:jsonnet.bzl", "jsonnet_test")
```

And add the test target:

```python
jsonnet_test(
    name = "test_framework_integration_test",
    src = "test_framework_test.jsonnet",
)
```

- [ ] **Step 4: Run the integration test**

Run: `bazel test //end2end:test_framework_integration_test`
Expected: Test passes (exit code 0), all 4 test functions pass.

- [ ] **Step 5: Run full test suite**

Run: `bazel test //...`
Expected: All tests pass, including the new integration test.

- [ ] **Step 6: Commit**

```bash
git add rules/jsonnet.bzl end2end/test_framework_test.jsonnet end2end/BUILD.bazel
git commit -m "feat: add jsonnet_test Bazel rule with integration test"
```

---

### Task 8: Test runner integration tests

**Files:**
- Create: `end2end/test_runner_failing_test.jsonnet`
- Create: `end2end/test_runner_non_object_test.jsonnet`
- Create: `end2end/run_failing_test.sh`
- Modify: `end2end/BUILD.bazel`

These tests verify edge cases: failing tests produce non-zero exit, non-object files error correctly, helper functions are ignored.

- [ ] **Step 1: Create a test file with intentional failures**

Create `end2end/test_runner_failing_test.jsonnet`:

```jsonnet
{
  testWillPass(): std.assertEqual(true, true),
  testWillFail(): std.assertEqual(1, 2),
  helperIgnored(x): x,
}
```

- [ ] **Step 2: Create a shell test wrapper for expected failure**

Create `end2end/run_failing_test.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

RUNNER="$1"
TEST_FILE="$2"

# Run the test runner; we expect it to fail (exit 1)
if "$RUNNER" -J . "$TEST_FILE"; then
    echo "ERROR: Expected test runner to exit non-zero, but it succeeded"
    exit 1
fi

echo "PASS: Test runner correctly reported failure"
exit 0
```

- [ ] **Step 3: Add test targets to end2end/BUILD.bazel**

```python
sh_test(
    name = "test_runner_failing_integration_test",
    srcs = ["run_failing_test.sh"],
    args = [
        "$(location //:jsonnet_test_runner)",
        "$(location test_runner_failing_test.jsonnet)",
    ],
    data = [
        "//:jsonnet_test_runner",
        "test_runner_failing_test.jsonnet",
    ],
)
```

- [ ] **Step 4: Run the tests**

Run: `bazel test //end2end:test_runner_failing_integration_test`
Expected: Test passes (the runner correctly exits non-zero for failing tests).

- [ ] **Step 5: Run full test suite**

Run: `bazel test //...`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add end2end/test_runner_failing_test.jsonnet end2end/run_failing_test.sh end2end/BUILD.bazel
git commit -m "test: add integration tests for test runner edge cases"
```
