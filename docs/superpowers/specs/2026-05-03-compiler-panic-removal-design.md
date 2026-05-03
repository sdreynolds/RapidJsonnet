# Compiler Panic Removal Design

**Date:** 2026-05-03

## Problem

`src/compiler.rs` contains 14 non-test `unwrap`/`expect` calls that would produce cryptic
Rust panics rather than actionable error messages. `src/jsonnet_compiler.rs` has one
additional `expect` in `main()` that is user-facing but has been scoped out. Three categories
of fixes are needed.

## Scope

`jsonnet_compiler.rs` line 26 is explicitly out of scope — leave it alone.

---

## Fix 1 — `declare_local` return type

### Current

```rust
fn declare_local(&mut self, name: String) -> Result<(), CompilerError>
```

Callers that need the resulting stack slot do a second lookup:

```rust
self.declare_local("__comp_result".to_string())?;
let result_slot = self.locals.last().unwrap().stack_slot;  // 12 of these
```

### Change

```rust
fn declare_local(&mut self, name: String) -> Result<usize, CompilerError>
```

Return `stack_slot` instead of `()`. The 12 two-line pairs collapse to:

```rust
let result_slot = self.declare_local("__comp_result".to_string())?;
```

The 5 call sites that don't need the slot (`<closure>`, `<self>`, `<super>`, loop
variables, parameter names) continue to work unchanged — `self.declare_local(...)?;`
silently discards the `usize`.

**17 call sites total. No behaviour change. No new error paths.**

---

## Fix 2 — `next.as_ref().unwrap()` in `super.` handling (line 687)

### Context

In `parse_prefix`, the `Token::Super` branch saves the current token as `next`, then
matches on it. Inside the `Some(Token::Dot)` arm, the code accesses `next.as_ref().unwrap().span`
to build an error span for a missing field name after `super.`.

The `unwrap()` is structurally safe (we are inside `Some(Token::Dot)`), but it reads
poorly and will panic cryptically if ever called out of that context.

### Change

Destructure the dot token directly in the match arm so `unwrap()` is not needed:

```rust
Some(Token::Dot) => {
    let dot_span = next.as_ref().map(|t| t.span.clone()).unwrap_or(0..0);
    // use dot_span in the error below
```

Or, since `next` is cloned and `Some` is guaranteed here, capture `dot_token` by
rebinding in the arm. Either way: no `unwrap()`.

**No behaviour change for valid inputs. Better span attribution when `super.` is
immediately followed by EOF.**

---

## Fix 3 — `expect` in `end_function` (line 3773)

### Context

`end_function` takes the enclosing scope back out of `self.enclosing`. This is always
`Some` because every `end_function` call is preceded by a successful `begin_function`,
and all fallible operations between them use `?` (so they return before reaching
`end_function` on error).

This cannot be triggered by user input. It can only fire if the compiler itself has
a mismatched `begin_function`/`end_function` pair — a programmer bug.

### Change

```rust
// Before
let enclosing = self.enclosing.take().expect("Must have enclosing scope");

// After
let enclosing = self.enclosing.take()
    .unwrap_or_else(|| unreachable!("end_function called without matching begin_function"));
```

`unreachable!` is the honest signal: "this branch cannot be reached; if it is, the
compiler has a bug."

---

## Fix 4 — End-to-end compilation error tests

The test runner (`run_test.sh`) marks any file containing `error`, `wrong`, `unmatched`,
`unexpected`, or `missing` in its filename as expected-to-fail (non-zero exit).

New files in `end2end/`:

| File | Error triggered | Key quality |
|---|---|---|
| `super_dot_missing_field_error.jsonnet` | `super .` with no identifier after dot, inside an object | Span points at dot, not blank |
| `local_missing_semicolon_error.jsonnet` | `local x = 1 x` (no `;` before body) | Points at `x` after value |
| `local_missing_equals_error.jsonnet` | `local x 1` (no `=`) | Points at `1` where `=` expected |
| `object_dynamic_key_missing_bracket_error.jsonnet` | `{ [expr: v }` — missing `]` | Points at `:` where `]` expected |
| `function_required_after_default_error.jsonnet` | `function(a=1, b) b` — required param after default | Points at `b` |
| `undefined_variable_error.jsonnet` | `foo + 1` | Span covers `foo` |
| `dollar_outside_object_error.jsonnet` | `$ + 1` at top level | `$` used outside object scope |

Each test only needs to exit non-zero and print a useful ariadne-formatted error. No
golden-file matching — the test framework just checks the exit code.

**BUILD.bazel is auto-covered by the existing glob that picks up all `*.jsonnet` files.**
