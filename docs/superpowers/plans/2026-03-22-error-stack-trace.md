# Error Stack Trace Visualization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display full error stack traces when import chains fail, showing both the root cause and the import call site(s) in a single multi-source ariadne report.

**Architecture:** Add `cause: Option<Box<ScanError>>` linked list to the existing error type. The VM's `force_value()` wraps import errors with the caller's location. `into_report()` walks the chain to build a single multi-file ariadne report. A helper in `main.rs` handles multi-source display at all 6 error report sites.

**Tech Stack:** Rust, ariadne (multi-source `sources()` API), Bazel

**Spec:** `docs/superpowers/specs/2026-03-22-error-stack-trace-design.md`

---

### Task 1: Add `cause` field to `ScanError` and a `new()` constructor

**Files:**
- Modify: `src/scanner.rs:62-67` (struct definition)
- Modify: `src/scanner.rs:69-93` (impl block)

There are ~450 `RuntimeError { span, message, source_id }` construction sites across `virtual_machine.rs` (246), `native.rs` (197), `scanner.rs` (4), `compiler.rs` (1), and `parser.rs` (2). Rather than adding `cause: None` to every one, add a constructor that defaults `cause` to `None`.

- [ ] **Step 1: Add `cause` field and `new()` constructor to `ScanError`**

In `src/scanner.rs`, change the struct and add a constructor:

```rust
#[derive(Debug, Clone)]
pub struct ScanError {
    pub span: Range<usize>,
    pub message: String,
    pub source_id: String,
    pub cause: Option<Box<ScanError>>,
}

impl ScanError {
    pub fn new(span: Range<usize>, message: String, source_id: String) -> Self {
        Self {
            span,
            message,
            source_id,
            cause: None,
        }
    }
    // ... existing methods unchanged
}
```

- [ ] **Step 2: Bulk-replace all `RuntimeError { span:, message:, source_id: }` with `RuntimeError::new()`**

Use a multiline perl regex to replace the 4-5 line struct literal pattern with the constructor call. The pattern captures field values (which may contain nested braces/parens like `format!(...)`) and reorders them into `::new()` args.

```bash
# For RuntimeError in virtual_machine.rs and native.rs:
perl -0777 -i -pe 's/RuntimeError\s*\{\s*\n\s*span:\s*(.*?),\s*\n\s*message:\s*(.*?),\s*\n\s*source_id:\s*(.*?),\s*\n\s*\}/RuntimeError::new($1, $2, $3)/gs' src/virtual_machine.rs src/native.rs

# For ScanError in scanner.rs, compiler.rs, parser.rs:
perl -0777 -i -pe 's/ScanError\s*\{\s*\n\s*span:\s*(.*?),\s*\n\s*message:\s*(.*?),\s*\n\s*source_id:\s*(.*?),\s*\n\s*\}/ScanError::new($1, $2, $3)/gs' src/scanner.rs src/compiler.rs src/parser.rs
```

After running, verify with `bazel build //...`. If any construction sites have unusual formatting (multi-line field values), the build will fail and those can be fixed manually.

Note: The three `force_value()` error paths that will get `cause` wrapping in Task 3 should NOT be converted to `::new()` — they will use the full struct literal with `cause: Some(...)`. Since those are modified in Task 3, convert them to `::new()` now (Task 1) and Task 3 will replace them with the full struct literal.

- [ ] **Step 3: Build to verify**

Run: `bazel build //...`
Expected: BUILD SUCCESS — all existing code compiles with the new field + constructor.

- [ ] **Step 4: Run tests to verify no regressions**

Run: `bazel test //...`
Expected: All previously-passing tests still pass. `greeting_test` still fails (not fixed yet).

- [ ] **Step 5: Commit**

```bash
git add src/scanner.rs src/virtual_machine.rs src/native.rs src/compiler.rs src/parser.rs
git commit -m "refactor: add cause field to ScanError with new() constructor"
```

---

### Task 2: Update `into_report()` for multi-source chain rendering

**Files:**
- Modify: `src/scanner.rs:78-92` (`into_report` method)

- [ ] **Step 1: Update `into_report()` to walk the cause chain**

Replace the existing `into_report` method:

```rust
pub fn into_report(&self) -> (Report<'static, (&str, Range<usize>)>, Vec<String>) {
    let red = ariadne::Color::Red;
    let yellow = ariadne::Color::Yellow;

    // Collect all errors in the chain (outermost first, root cause last)
    let mut chain: Vec<&ScanError> = Vec::new();
    let mut current = self;
    loop {
        chain.push(current);
        match &current.cause {
            Some(next) => current = next,
            None => break,
        }
    }

    // Root cause is the last element
    let root_cause = chain.last().unwrap();

    // Build report with root cause as primary
    let mut builder = Report::build(
        ReportKind::Error,
        (root_cause.source_id.as_str(), root_cause.span.clone()),
    )
    .with_message(&root_cause.message)
    .with_label(
        Label::new((root_cause.source_id.as_str(), root_cause.span.clone()))
            .with_message(&root_cause.message)
            .with_color(red),
    );

    // Add caller frames as additional labels (all except the last/root cause)
    for frame in chain.iter().take(chain.len().saturating_sub(1)) {
        builder = builder.with_label(
            Label::new((frame.source_id.as_str(), frame.span.clone()))
                .with_message(&frame.message)
                .with_color(yellow),
        );
    }

    // Collect unique source IDs
    let mut source_ids: Vec<String> = Vec::new();
    for frame in &chain {
        if !source_ids.contains(&frame.source_id) {
            source_ids.push(frame.source_id.clone());
        }
    }

    (builder.finish(), source_ids)
}
```

- [ ] **Step 2: Build to verify**

Run: `bazel build //:main`
Expected: BUILD FAILURE — `main.rs` still expects the old `into_report()` signature. This is expected; we fix it in Task 4.

- [ ] **Step 3: Commit** (hold until Task 4 compiles, or commit together with Task 4)

---

### Task 3: Wrap import errors in `force_value()`

**Files:**
- Modify: `src/virtual_machine.rs` — three error paths in `force_value()` method

- [ ] **Step 1: Wrap compile error (around line 1327-1342)**

Replace the existing compile error handling:

```rust
Err(e) => {
    self.memory_manager.pop_external_roots();
    self.memory_manager
        .load_import(import_idx)
        .evaluating
        .set(false);
    return Err(RuntimeError {
        span: self.get_current_span(),
        message: format!(
            "while evaluating import \"{}\"",
            target_path_str
        ),
        source_id: self.current_chunk().source_id.to_string(),
        cause: Some(Box::new(e)),
    });
}
```

Note: this error uses the full struct literal (not `::new()`) because it sets `cause`.

- [ ] **Step 2: Wrap sub-VM runtime error (around line 1367-1373)**

Replace the existing runtime error pass-through:

```rust
Err(e) => {
    self.memory_manager
        .load_import_mut(import_idx)
        .evaluating
        .set(false);
    Err(RuntimeError {
        span: self.get_current_span(),
        message: format!(
            "while evaluating import \"{}\"",
            target_path_str
        ),
        source_id: self.current_chunk().source_id.to_string(),
        cause: Some(Box::new(e)),
    })
}
```

- [ ] **Step 3: Wrap recursive `force_value` error (around line 1361)**

The current line is:
```rust
let forced = self.force_value(evaluated_value)?;
```

Replace with explicit match to wrap the error:

```rust
let forced = match self.force_value(evaluated_value) {
    Ok(v) => v,
    Err(e) => {
        let import = self.memory_manager.load_import_mut(import_idx);
        import.evaluating.set(false);
        return Err(RuntimeError {
            span: self.get_current_span(),
            message: format!(
                "while evaluating import \"{}\"",
                target_path_str
            ),
            source_id: self.current_chunk().source_id.to_string(),
            cause: Some(Box::new(e)),
        });
    }
};
```

- [ ] **Step 4: Commit** (hold until Task 4 compiles, or commit together)

---

### Task 4: Update `main.rs` for multi-source error display

**Files:**
- Modify: `src/main.rs` — all 6 error report call sites + add helper function + add import

- [ ] **Step 1: Add `sources` import**

At the top of `src/main.rs`, change:
```rust
use ariadne::Source;
```
to:
```rust
use ariadne::{sources, Source};
```

- [ ] **Step 2: Add helper function**

Add this helper function after `compile_and_execute_quiet` (after line 269):

```rust
fn print_error_report(
    error: &scanner::ScanError,
    primary_source_id: &str,
    primary_content: &str,
    use_stderr: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (report, source_ids) = error.into_report();
    let srcs: Vec<(String, ariadne::Source)> = source_ids
        .iter()
        .map(|sid| {
            let content = if sid == primary_source_id {
                primary_content.to_string()
            } else {
                fs::read_to_string(sid).unwrap_or_else(|_| "<file not found>".to_string())
            };
            (sid.clone(), ariadne::Source::from(content))
        })
        .collect();
    if use_stderr {
        report.eprint(sources(srcs))?;
    } else {
        report.print(sources(srcs))?;
    }
    Ok(())
}
```

- [ ] **Step 3: Update `compile_and_execute_quiet` — runtime error (line 241-253)**

Replace:
```rust
Err(runtime_error) => {
    let error_source_id = runtime_error.source_id.clone();
    let error_content = if error_source_id == source_id {
        content.to_string()
    } else {
        fs::read_to_string(&error_source_id)
            .unwrap_or_else(|_| "<file not found>".to_string())
    };
    let error_source = Source::from(error_content);
    let report = runtime_error.into_report();
    report.eprint((error_source_id.as_str(), error_source))?;
    Err(Box::new(MainError::RuntimeError))
}
```

With:
```rust
Err(runtime_error) => {
    print_error_report(&runtime_error, source_id, content, true)?;
    Err(Box::new(MainError::RuntimeError))
}
```

- [ ] **Step 4: Update `compile_and_execute_quiet` — compile error (line 255-267)**

Replace:
```rust
Err(compile_error) => {
    let error_source_id = compile_error.source_id.clone();
    let error_content = if error_source_id == source_id {
        content.to_string()
    } else {
        fs::read_to_string(&error_source_id)
            .unwrap_or_else(|_| "<file not found>".to_string())
    };
    let error_source = Source::from(error_content);
    let report = compile_error.into_report();
    report.eprint((error_source_id.as_str(), error_source))?;
    Err(Box::new(MainError::CompilerError))
}
```

With:
```rust
Err(compile_error) => {
    print_error_report(&compile_error, source_id, content, true)?;
    Err(Box::new(MainError::CompilerError))
}
```

- [ ] **Step 5: Update `compile_and_execute` — runtime error (line 303-316)**

Replace:
```rust
Err(runtime_error) => {
    println!("❌ Runtime error during execution:");
    let error_source_id = runtime_error.source_id.clone();
    let error_content = if error_source_id == source_id {
        content.to_string()
    } else {
        fs::read_to_string(&error_source_id)
            .unwrap_or_else(|_| "<file not found>".to_string())
    };
    let error_source = Source::from(error_content);
    let report = runtime_error.into_report();
    report.print((error_source_id.as_str(), error_source))?;
    Err(Box::new(MainError::RuntimeError))
}
```

With:
```rust
Err(runtime_error) => {
    println!("❌ Runtime error during execution:");
    print_error_report(&runtime_error, source_id, content, false)?;
    Err(Box::new(MainError::RuntimeError))
}
```

- [ ] **Step 6: Update `compile_and_execute` — compile error (line 319-332)**

Replace:
```rust
Err(compile_error) => {
    println!("❌ Compilation failed:");
    let error_source_id = compile_error.source_id.clone();
    let error_content = if error_source_id == source_id {
        content.to_string()
    } else {
        fs::read_to_string(&error_source_id)
            .unwrap_or_else(|_| "<file not found>".to_string())
    };
    let error_source = Source::from(error_content);
    let report = compile_error.into_report();
    report.print((error_source_id.as_str(), error_source))?;
    Err(Box::new(MainError::CompilerError))
}
```

With:
```rust
Err(compile_error) => {
    println!("❌ Compilation failed:");
    print_error_report(&compile_error, source_id, content, false)?;
    Err(Box::new(MainError::CompilerError))
}
```

- [ ] **Step 7: Update `process_repl_input` — runtime error (line 412-425)**

Replace:
```rust
Err(runtime_error) => {
    println!("❌ Runtime error during execution:");
    let error_source_id = runtime_error.source_id.clone();
    let error_content = if error_source_id == source_id {
        content.to_string()
    } else {
        fs::read_to_string(&error_source_id)
            .unwrap_or_else(|_| "<file not found>".to_string())
    };
    let error_source = Source::from(error_content);
    let report = runtime_error.into_report();
    let _ = report.print((error_source_id.as_str(), error_source));
    ReplResult::Error
}
```

With:
```rust
Err(runtime_error) => {
    println!("❌ Runtime error during execution:");
    let _ = print_error_report(&runtime_error, source_id, content, false);
    ReplResult::Error
}
```

- [ ] **Step 8: Update `process_repl_input` — compile error (line 433-444)**

Replace:
```rust
println!("❌ Compilation failed:");
let error_source_id = compile_error.source_id.clone();
let error_content = if error_source_id == source_id {
    content.to_string()
} else {
    fs::read_to_string(&error_source_id)
        .unwrap_or_else(|_| "<file not found>".to_string())
};
let error_source = Source::from(error_content);
let report = compile_error.into_report();
let _ = report.print((error_source_id.as_str(), error_source));
ReplResult::Error
```

With:
```rust
println!("❌ Compilation failed:");
let _ = print_error_report(&compile_error, source_id, content, false);
ReplResult::Error
```

- [ ] **Step 9: Build to verify everything compiles**

Run: `bazel build //...`
Expected: BUILD SUCCESS

- [ ] **Step 10: Run all tests**

Run: `bazel test //...`
Expected: All previously-passing tests still pass. `greeting_test` still fails (test expects success but the file has an error).

- [ ] **Step 11: Commit Tasks 2-4 together**

```bash
git add src/scanner.rs src/virtual_machine.rs src/main.rs
git commit -m "feat: display full error stack traces for import chains

Walk the ScanError cause chain to build a single multi-source ariadne
report showing both the root cause and the import call site(s)."
```

---

### Task 5: Rename greeting test to expect failure

**Files:**
- Rename: `end2end/greeting.jsonnet` → `end2end/greeting_error.jsonnet`

- [ ] **Step 1: Rename the test file**

```bash
git mv end2end/greeting.jsonnet end2end/greeting_error.jsonnet
```

The `BUILD.bazel` glob automatically picks up the new filename. Since "error" is in the name, `run_test.sh` will expect failure (exit code non-zero).

- [ ] **Step 2: Run the greeting error test**

Run: `bazel test //end2end:greeting_error_test`
Expected: PASS — the test now expects failure, and the binary correctly exits non-zero with a multi-source error report.

- [ ] **Step 3: Run all tests**

Run: `bazel test //...`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git commit -m "test: rename greeting test to expect failure for undefined variable error"
```

Note: `git mv` in Step 1 already stages both the add and delete, so no additional `git add` needed.
