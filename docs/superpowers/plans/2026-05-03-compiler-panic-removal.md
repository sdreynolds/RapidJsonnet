# Compiler Panic Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove all non-test `unwrap`/`expect` panics from `src/compiler.rs` and add end-to-end tests that exercise compilation error paths with clear, well-spanned error messages.

**Architecture:** Three mechanical code fixes in `compiler.rs` (return type change, unwrap removal, unreachable! substitution) followed by seven new `.jsonnet` test files in `end2end/` that each trigger a distinct compile-time error and exit non-zero. No new modules, no new error types, no behaviour changes for valid programs.

**Tech Stack:** Rust, Bazel (`bazel test`), ariadne (error reporting), existing `end2end/run_test.sh` test harness.

---

### Task 1: Change `declare_local` to return the stack slot

**Files:**
- Modify: `src/compiler.rs` — `fn declare_local` signature + 12 call sites

The function currently returns `Result<(), CompilerError>`. Changing it to return
`Result<usize, CompilerError>` (the slot index) eliminates 12 `locals.last().unwrap()`
calls. There are 28 call sites total; 16 don't use the returned value and need no
syntactic change.

- [ ] **Step 1: Confirm baseline tests pass**

```bash
bazel test //:compiler_test
```
Expected: all tests pass.

- [ ] **Step 2: Change `declare_local` return type and body**

In `src/compiler.rs` at line ~3616, change:

```rust
fn declare_local(&mut self, name: String) -> Result<(), CompilerError> {
    // Check for duplicate in current scope
    for local in self.locals.iter().rev() {
        if local.depth < self.scope_depth {
            break; // Reached outer scope
        }
        if local.name == name {
            return Err(self.make_error(
                self.current_span(),
                format!("Variable '{}' already declared in this scope", name),
            ));
        }
    }

    let stack_slot = self.locals.len() + self.anon_stack_depth;

    self.locals.push(Local {
        name,
        depth: self.scope_depth,
        stack_slot,
        is_captured: false,
    });

    Ok(())
}
```

To:

```rust
fn declare_local(&mut self, name: String) -> Result<usize, CompilerError> {
    // Check for duplicate in current scope
    for local in self.locals.iter().rev() {
        if local.depth < self.scope_depth {
            break; // Reached outer scope
        }
        if local.name == name {
            return Err(self.make_error(
                self.current_span(),
                format!("Variable '{}' already declared in this scope", name),
            ));
        }
    }

    let stack_slot = self.locals.len() + self.anon_stack_depth;

    self.locals.push(Local {
        name,
        depth: self.scope_depth,
        stack_slot,
        is_captured: false,
    });

    Ok(stack_slot)
}
```

- [ ] **Step 3: Collapse the 12 two-line pairs**

Each of the following pairs appears in this pattern:
```rust
self.declare_local("some_name".to_string())?;
let foo_slot = self.locals.last().unwrap().stack_slot;
```
Replace each with a single line:
```rust
let foo_slot = self.declare_local("some_name".to_string())?;
```

The 12 locations and their replacement variable names:

| Line (approx) | Old second line variable | Replacement |
|---|---|---|
| 2713–2714 | `result_slot` | `let result_slot = self.declare_local("__comp_result".to_string())?;` |
| 2873–2874 | `source_slot` | `let source_slot = self.declare_local("__comp_source".to_string())?;` |
| 2882–2883 | `length_slot` | `let length_slot = self.declare_local("__comp_length".to_string())?;` |
| 2886–2887 | `counter_slot` | `let counter_slot = self.declare_local("__comp_counter".to_string())?;` |
| 2898–2899 | inline `Some(...)` | `Some(self.declare_local("__comp_hoisted_source".to_string())?)` |
| 3102–3103 | `result_slot` | `let result_slot = self.declare_local("__comp_result".to_string())?;` |
| 3228–3229 | `source_slot` | `let source_slot = self.declare_local("__comp_source".to_string())?;` |
| 3238–3239 | `length_slot` | `let length_slot = self.declare_local("__comp_length".to_string())?;` |
| 3243–3244 | `counter_slot` | `let counter_slot = self.declare_local("__comp_counter".to_string())?;` |
| 3256–3257 | inline `Some(...)` | `Some(self.declare_local("__comp_hoisted_source".to_string())?)` |
| 3470–3471 | push into `slots` vec | `slots.push(self.declare_local(name.clone())?);` |
| 3957–3958 | push into `param_slots` vec | `param_slots.push(self.declare_local(param.name.clone())?);` |

The 16 call sites that do NOT use the returned slot need no changes — `self.declare_local(...)?;` discards the `usize` silently.

- [ ] **Step 4: Build and verify no compile errors**

```bash
bazel build //...
```
Expected: clean build, no type errors.

- [ ] **Step 5: Run compiler tests**

```bash
bazel test //:compiler_test
```
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/compiler.rs
git commit -m "refactor: declare_local returns stack slot, removing 12 unwrap() calls"
```

---

### Task 2: Remove `unwrap()` from `super.` EOF error path (line ~687)

**Files:**
- Modify: `src/compiler.rs` — `parse_prefix`, `Token::Super` branch

When `super.` is at end of file, `current_token()` returns `None` and the error message
builder calls `next.as_ref().unwrap().span`. `next` is the dot token and is guaranteed
`Some` inside the `Some(Token::Dot)` arm, but `unwrap()` is still a code smell.
Replace with `map`+`unwrap_or_default`.

- [ ] **Step 1: Locate the exact lines**

Find the block in `src/compiler.rs`:

```rust
Some(Token::Dot) => {
    self.parser.advance()?; // consume '.'
    let field_token =
        self.parser.current_token().cloned().ok_or_else(|| {
            self.make_error(
                next.as_ref().unwrap().span.clone(),
                "Expected field name after 'super.'".to_string(),
            )
        })?;
```

- [ ] **Step 2: Replace with safe span extraction**

```rust
Some(Token::Dot) => {
    self.parser.advance()?; // consume '.'
    let field_token =
        self.parser.current_token().cloned().ok_or_else(|| {
            self.make_error(
                next.as_ref().map(|t| t.span.clone()).unwrap_or(0..0),
                "Expected field name after 'super.'".to_string(),
            )
        })?;
```

`unwrap_or(0..0)` is the standard fallback span used throughout the compiler when no
better location is available (`Range<usize>` does not implement `Default`).
In practice `next` is always `Some` here, so the actual span is always used.

- [ ] **Step 3: Build and test**

```bash
bazel build //...
bazel test //:compiler_test
```
Expected: clean build, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/compiler.rs
git commit -m "refactor: replace unwrap() in super-dot EOF error with safe map"
```

---

### Task 3: Replace `expect` with `unreachable!` in `end_function` (line ~3773)

**Files:**
- Modify: `src/compiler.rs` — `fn end_function`

`end_function` is always called after a successful `begin_function`; all fallible
operations between them use `?` and short-circuit before `end_function` is reached.
`self.enclosing` is always `Some` here. Replacing `expect` with `unreachable!`
documents this invariant honestly — if it ever fires, it is a compiler bug, not user
input.

- [ ] **Step 1: Locate the exact line**

Find in `fn end_function`:

```rust
let enclosing = self.enclosing.take().expect("Must have enclosing scope");
```

- [ ] **Step 2: Replace with `unreachable!`**

```rust
let enclosing = self.enclosing.take()
    .unwrap_or_else(|| unreachable!("end_function called without matching begin_function"));
```

- [ ] **Step 3: Build and test**

```bash
bazel build //...
bazel test //:compiler_test
```
Expected: clean build, all tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/compiler.rs
git commit -m "refactor: replace expect() with unreachable!() in end_function invariant"
```

---

### Task 4: End-to-end test — `super` dot with non-identifier

**Files:**
- Create: `end2end/super_dot_missing_field_error.jsonnet`

The `run_test.sh` harness marks any file containing `error` in its name as
expected-to-fail (must exit non-zero). The `BUILD.bazel` glob picks it up automatically.

- [ ] **Step 1: Create the test file**

```jsonnet
{ a: super.42 }
```

`42` is a number token, not an identifier. The compiler rejects it at `super.` handling
with "Expected field name after 'super.'" and attributes the span to `42`.

- [ ] **Step 2: Run the test and verify it fails with a good error message**

```bash
bazel run //:main_stress -- end2end/super_dot_missing_field_error.jsonnet
```

Expected output (stderr, ariadne-formatted):
```
Error: Expected field name after 'super.'
  --> end2end/super_dot_missing_field_error.jsonnet:1:12
   |
 1 | { a: super.42 }
   |            ^^ 42 is not a valid field name
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:super_dot_missing_field_error_test
```
Expected: PASS (test expects failure, binary exits non-zero).

- [ ] **Step 4: Commit**

```bash
git add end2end/super_dot_missing_field_error.jsonnet
git commit -m "test: add e2e test for super-dot with non-identifier field name"
```

---

### Task 5: End-to-end test — `local` missing semicolon

**Files:**
- Create: `end2end/local_missing_semicolon_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
local x = 1 x
```

After parsing the binding `x = 1`, the parser sees `x` (identifier) where it expects
`,` or `;`. Error: "Expected ',' or ';' in local statement" with span on the second `x`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/local_missing_semicolon_error.jsonnet
```

Expected output:
```
Error: Expected ',' or ';' in local statement
  --> end2end/local_missing_semicolon_error.jsonnet:1:13
   |
 1 | local x = 1 x
   |             ^ expected ',' or ';' here
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:local_missing_semicolon_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/local_missing_semicolon_error.jsonnet
git commit -m "test: add e2e test for local binding missing semicolon"
```

---

### Task 6: End-to-end test — `local` missing equals sign

**Files:**
- Create: `end2end/local_missing_equals_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
local x 1; x
```

After the binding name `x`, the parser expects `=` and finds `1`. Error: "Expected '='
after variable name" with span on `1`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/local_missing_equals_error.jsonnet
```

Expected output:
```
Error: Expected '=' after variable name
  --> end2end/local_missing_equals_error.jsonnet:1:9
   |
 1 | local x 1; x
   |         ^ expected '=' here
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:local_missing_equals_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/local_missing_equals_error.jsonnet
git commit -m "test: add e2e test for local binding missing equals sign"
```

---

### Task 7: End-to-end test — object dynamic key missing closing bracket

**Files:**
- Create: `end2end/object_dynamic_key_missing_bracket_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
{ [1 + 2: "val" }
```

After parsing the dynamic key expression `1 + 2`, the parser expects `]` and finds `:`.
Error: "Expected ']' after dynamic object key" with span on `:`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/object_dynamic_key_missing_bracket_error.jsonnet
```

Expected output:
```
Error: Expected ']' after dynamic object key
  --> end2end/object_dynamic_key_missing_bracket_error.jsonnet:1:9
   |
 1 | { [1 + 2: "val" }
   |         ^ expected ']' to close dynamic key
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:object_dynamic_key_missing_bracket_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/object_dynamic_key_missing_bracket_error.jsonnet
git commit -m "test: add e2e test for object dynamic key missing closing bracket"
```

---

### Task 8: End-to-end test — required parameter after default parameter

**Files:**
- Create: `end2end/function_required_after_default_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
function(a=1, b) b
```

In `parse_parameter_list`, after seeing `a=1` (sets `seen_default = true`), parameter
`b` has no default. Error: "Required parameter cannot follow parameter with default"
with span on `b`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/function_required_after_default_error.jsonnet
```

Expected output:
```
Error: Required parameter cannot follow parameter with default
  --> end2end/function_required_after_default_error.jsonnet:1:15
   |
 1 | function(a=1, b) b
   |               ^ 'b' has no default but follows 'a' which does
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:function_required_after_default_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/function_required_after_default_error.jsonnet
git commit -m "test: add e2e test for required param following default param"
```

---

### Task 9: End-to-end test — undefined variable

**Files:**
- Create: `end2end/undefined_variable_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
foo + 1
```

`foo` is not defined as a local, upvalue, or built-in. Error: "Undefined variable 'foo'"
with span covering `foo`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/undefined_variable_error.jsonnet
```

Expected output:
```
Error: Undefined variable 'foo'
  --> end2end/undefined_variable_error.jsonnet:1:1
   |
 1 | foo + 1
   | ^^^ not defined in this scope
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:undefined_variable_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/undefined_variable_error.jsonnet
git commit -m "test: add e2e test for undefined variable reference"
```

---

### Task 10: End-to-end test — `$` outside object scope

**Files:**
- Create: `end2end/dollar_outside_object_error.jsonnet`

- [ ] **Step 1: Create the test file**

```jsonnet
$ + 1
```

`$` is resolved via `resolve_local("$")` and `resolve_upvalue("$")`. At top level
there is no enclosing object so neither finds a binding. Error: "'$' used outside of
object scope" with span on `$`.

- [ ] **Step 2: Run and verify error message**

```bash
bazel run //:main_stress -- end2end/dollar_outside_object_error.jsonnet
```

Expected output:
```
Error: '$' used outside of object scope
  --> end2end/dollar_outside_object_error.jsonnet:1:1
   |
 1 | $ + 1
   | ^ '$' is only available inside an object literal
```
Expected exit code: non-zero.

- [ ] **Step 3: Run via Bazel test**

```bash
bazel test //end2end:dollar_outside_object_error_test
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add end2end/dollar_outside_object_error.jsonnet
git commit -m "test: add e2e test for dollar-sign outside object scope"
```

---

### Final verification

- [ ] **Run all end2end tests**

```bash
bazel test //end2end/...
```
Expected: all pass (new tests pass because they correctly exit non-zero; existing tests
are unaffected by the compiler refactors).

- [ ] **Run full test suite**

```bash
bazel test //...
```
Expected: all pass.
