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

The method:
- Walks the cause chain to collect all errors
- Uses the **root cause** (deepest error) as the report's primary location and message
- Adds each **intermediate frame** as an additional label with "while evaluating import" style message
- Returns all source IDs so callers know which files to load

### 3. VM `force_value()` changes (`virtual_machine.rs`)

Three error paths in `force_value()` are updated:

1. **Compile error** (import target fails to compile) — creates a caller-location error with the compile error as `cause`
2. **Runtime error from sub-VM** (import target fails at runtime) — creates a caller-location error with the runtime error as `cause`
3. **Runtime error from recursive `force_value`** — same wrapping pattern

Each creates:
```rust
RuntimeError {
    span: self.get_current_span(),  // caller's import statement
    message: format!("while evaluating import \"{}\"", target_path_str),
    source_id: self.current_chunk().source_id.to_string(),
    cause: Some(Box::new(original_error)),
}
```

### 4. `main.rs` error display changes

Both runtime and compile error branches updated to:
1. Call `into_report()` → `(report, source_ids)`
2. Load all referenced source files (using existing file content for the primary file, `fs::read_to_string` for others)
3. Pass via `sources(...)` to `report.eprint()`

### Expected output

For `greeting.jsonnet` importing `utils.libsonnet` with undefined variable `namesda`:

```
Error: undefined variable 'namesda'
   ╭─[utils.libsonnet:2:27]
   │
 2 │   greet(name):: "Hello, " + namesda + "!",
   │                              ^^^^^^^ undefined variable 'namesda'
   │
   ╭─[greeting.jsonnet:1:21]
   │
 1 │ local utils = import "utils.libsonnet";
   │                     ──────────────────── while evaluating import "utils.libsonnet"
───╯
```

Root cause at the top (report primary), caller frames below as additional labels.

## Files to modify

1. `src/scanner.rs` — `ScanError` struct + `into_report()`
2. `src/virtual_machine.rs` — `force_value()` error wrapping
3. `src/main.rs` — multi-source error display
