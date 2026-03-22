# Error Stack Trace Visualization

## Problem

When an imported file has an error (e.g., `greeting.jsonnet` imports `utils.libsonnet` which references an undefined variable), only the innermost error location is shown. The user has no visibility into the import chain that led to the error. The `//end2end:greeting_test` fails because of this — the error message only points to `utils.libsonnet` without showing that `greeting.jsonnet` triggered the import.

## Solution

Add a `cause` chain to `ScanError` and render the full error chain as a single multi-source ariadne report.

## Design

### 1. `ScanError` changes (`scanner.rs`)

Add a `cause` field:

```rust
pub struct ScanError {
    pub span: Range<usize>,
    pub message: String,
    pub source_id: String,
    pub cause: Option<Box<ScanError>>,
}
```

All existing error construction sites continue with `cause: None`. Only the VM's `force_value()` sets `cause` when wrapping import errors.

### 2. `into_report()` changes (`scanner.rs`)

Change signature to return both the report and the list of source IDs referenced:

```rust
pub fn into_report(&self) -> (Report<'static, (&str, Range<usize>)>, Vec<String>)
```

Algorithm:
1. Walk the `cause` chain collecting all errors into a `Vec` (outermost first)
2. The **last element** (root cause / deepest error) becomes the report's primary location and message via `Report::build()`
3. Iterate remaining elements (callers) and add each as a `Label` with its own `(source_id, span)` and "while evaluating import" message
4. The root cause also gets a label for its span
5. Collect all unique `source_id` values into the returned `Vec<String>`

Note: ariadne clones span ID strings internally during `finish()`, so the `'static` return lifetime is safe despite borrowing from `&self` during construction.

### 3. VM `force_value()` changes (`virtual_machine.rs`)

There are five error paths in `force_value()`. Three get `cause` wrapping; two do not:

**Wrapped (add `cause`):**
1. **Compile error** (~line 1333) — import target fails to compile. Currently formats with `{:?}` losing the structured error. Wrap with caller location + `cause`.
2. **Sub-VM runtime error** (~line 1367) — import target fails at runtime. Currently passes through raw `Err(e)` with no caller context. Wrap with caller location + `cause`.
3. **Recursive `force_value` error** (~line 1361) — propagated via `?`. Wrap with caller location + `cause`.

**Not wrapped (unchanged):**
4. **Cyclic import** (~line 1230) — terminal error, already reports at the caller's span.
5. **File read errors** (~lines 1291, 1312) — terminal errors, already report at the caller's span with descriptive messages.

Each wrapped error creates:
```rust
RuntimeError {
    span: self.get_current_span(),  // caller's import statement
    message: format!("while evaluating import \"{}\"", target_path_str),
    source_id: self.current_chunk().source_id.to_string(),
    cause: Some(Box::new(original_error)),
}
```

### 4. `main.rs` error display changes

Six call sites need updating (all locations that call `.eprint()` or `.print()` on error reports):

**Quiet mode (`execute_file`, uses `eprint`):**
- Line 251 — runtime error
- Line 265 — compile error

**Verbose mode (`compile_and_execute`, uses `print`):**
- Line 314 — runtime error
- Line 330 — compile error

**REPL mode (`repl_eval`, uses `print`):**
- Line 423 — runtime error
- Line 443 — compile error

All six follow the same pattern:

```rust
let (report, source_ids) = error.into_report();
let srcs: Vec<(String, Source)> = source_ids.iter().map(|sid| {
    let content = if sid == source_id {
        content.to_string()
    } else {
        fs::read_to_string(sid).unwrap_or_else(|_| "<file not found>".to_string())
    };
    (sid.clone(), Source::from(content))
}).collect();
report.eprint(sources(srcs))?;  // or .print() for verbose/REPL
```

Extract a helper function to avoid duplicating this across all six sites.

### 5. Test changes (`end2end/`)

Rename `greeting.jsonnet` to `greeting_error.jsonnet` so `run_test.sh` expects failure (the "error" pattern in the filename triggers `EXPECTED_FAIL=1`). This test validates that the error stack trace includes both source files.

### Expected output

For `greeting_error.jsonnet` importing `utils.libsonnet` with undefined variable `namesda`:

```
Error: undefined variable 'namesda'
   ╭─[utils.libsonnet:2:27]
   │
 2 │   greet(name):: "Hello, " + namesda + "!",
   │                              ^^^^^^^ undefined variable 'namesda'
   │
   ╭─[greeting_error.jsonnet:1:21]
   │
 1 │ local utils = import "utils.libsonnet";
   │                     ──────────────────── while evaluating import "utils.libsonnet"
───╯
```

Root cause at the top (report primary), caller frames below as additional labels.

### Edge cases

- **Deep import chains** (A→B→C→D where D errors): naturally supported by the linked list. All frames rendered. No depth limit for v1.
- **Same-file errors**: if a cause chain has adjacent frames with the same `source_id`, ariadne handles this fine — labels from the same file are grouped together visually.

## Files to modify

1. `src/scanner.rs` — `ScanError` struct + `into_report()`
2. `src/virtual_machine.rs` — `force_value()` error wrapping
3. `src/main.rs` — multi-source error display + helper function
4. `end2end/greeting.jsonnet` — rename to `greeting_error.jsonnet`
