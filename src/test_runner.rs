use ariadne::sources;
use chunk::{ClosureIndex, FieldVisibility, ObjectIndex, RuntimeError, Value};
use coverage::CoverageCollector;
use memory_manager::MemoryManager;
use std::fs;
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
    super_obj: Option<ObjectIndex>, // base_object of the node that defined this field
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

    let mut fields = Vec::new();

    // Walk the base_object chain to collect all test fields
    let mut curr = Some(obj_idx);
    let mut seen_names = std::collections::HashSet::new();
    while let Some(idx) = curr {
        let obj = memory_manager.load_object(idx);
        let base = obj.base_object;
        for (key_idx, field) in &obj.properties {
            // Skip hidden fields
            if matches!(field.visibility, FieldVisibility::Hidden) {
                continue;
            }

            let name = memory_manager.load_string(*key_idx).to_string();
            if !name.starts_with("test") && !name.starts_with("skip_test") {
                continue;
            }
            if !seen_names.insert(name.clone()) {
                continue; // shallower node already added this field
            }

            // All object field values should be thunks (Closures with arity=2)
            match field.value {
                Value::Closure(ci) => {
                    fields.push(DiscoveredField {
                        name,
                        thunk_closure: ci,
                        super_obj: base,
                    });
                }
                _ => {
                    // Raw values are not expected for function fields
                    return Err(TestRunnerError::NotAFunction { name });
                }
            }
        }
        curr = base;
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
    test_filter: Option<String>,
    primary_source_id: &str,
    primary_content: &str,
) -> Result<(bool, Option<CoverageCollector>), TestRunnerError> {
    let (obj_idx, mut fields) = discover_test_fields(top_level_value, vm.memory_manager())?;

    if let Some(filter) = test_filter {
        fields.retain(|f| f.name.contains(&filter));
    }

    // Root the top-level object to prevent GC from collecting it (and its field
    // thunks) between test calls. Under stress_gc, GC runs aggressively and would
    // otherwise sweep the thunk closures we haven't forced yet.
    vm.push_external_roots(vec![*top_level_value]);

    let mut results = Vec::with_capacity(fields.len());

    let mut accumulated_coverage: Option<CoverageCollector> = None;

    for field in &fields {
        reporter.on_test_start(&field.name);

        if field.name.starts_with("skip_test") {
            let result = TestResult {
                name: field.name.clone(),
                outcome: TestOutcome::Skip,
            };
            reporter.on_test_complete(&result);
            results.push(result);
            continue;
        }

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
            Err(e) => {
                let rich_message = build_error_report(&e, primary_source_id, primary_content);
                TestOutcome::Fail {
                    message: rich_message,
                }
            }
        };

        let result = TestResult {
            name: field.name.clone(),
            outcome,
        };

        reporter.on_test_complete(&result);
        if collect_coverage {
            if let Some(test_coverage) = vm.take_coverage() {
                match &mut accumulated_coverage {
                    Some(acc) => acc.merge(test_coverage),
                    None => accumulated_coverage = Some(test_coverage),
                }
            }
        }
        results.push(result);
    }

    reporter.on_suite_complete(&results);

    // Release the GC roots
    vm.pop_external_roots();

    let all_passed = results
        .iter()
        .all(|r| !matches!(r.outcome, TestOutcome::Fail { .. }));
    Ok((all_passed, accumulated_coverage))
}

fn build_error_report(
    error: &RuntimeError,
    primary_source_id: &str,
    primary_content: &str,
) -> String {
    let (report, source_ids) = error.into_report();
    let srcs: Vec<(String, String)> = source_ids
        .iter()
        .map(|sid| {
            let content = if sid == primary_source_id {
                primary_content.to_string()
            } else {
                fs::read_to_string(sid).unwrap_or_else(|_| "<file not found>".to_string())
            };
            (sid.clone(), content)
        })
        .collect();

    let mut out = Vec::new();
    report.write(sources(srcs), &mut out).unwrap();
    String::from_utf8_lossy(&out).trim().to_string()
}
