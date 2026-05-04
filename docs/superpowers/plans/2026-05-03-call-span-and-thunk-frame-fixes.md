# Call Span and Thunk Frame Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix two stack trace quality issues: (1) call-site spans cover only `()` instead of the function name, and (2) a spurious thunk frame with a stale end-of-file span appears as the outermost stack trace entry.

**Architecture:** Two independent fixes in separate files. Task 1 fixes the compiler (`src/compiler.rs`) by tracking where the callee expression starts (`lhs_span_start` field) so the Call opcode's span covers the full call expression including the function name. Task 2 fixes the VM (`src/virtual_machine.rs`) by skipping `call_site` assignment for thunk closures in `call_closure` — thunks are lazy-evaluation internals, not user-written function calls, and they get stale call_sites from `instruction_start_ip` left over from the previous interpret cycle.

**Tech Stack:** Rust, Bazel/rules_rust, ariadne 0.5.0

---

### Task 1: Fix call-site span to cover the callee name

**Files:**
- Modify: `src/compiler.rs:172-209` — add `lhs_span_start` field to `Compiler` struct and initialize it
- Modify: `src/compiler.rs:260-303` — set and restore `lhs_span_start` in `parse_expr`
- Modify: `src/compiler.rs:1499` — use `self.lhs_span_start` instead of `token.span.start`

**Background:** When parsing a function call like `f(1)`, the Pratt parser enters `parse_infix` with the `(` as the current token. At that point, the callee `f` has already been compiled by `parse_prefix`. The current code uses `token.span.start` (position of `(`) as the start of the call span. After this fix, the span will start at the beginning of the callee expression, so `f(1)` shows in error reports instead of `(1)`. The fix works by recording the start position of the expression before `parse_prefix` is called, then restoring it after (since nested `parse_expr` calls inside `parse_prefix` may overwrite the field).

- [ ] **Step 1: Write a failing test that asserts the call span starts at the callee name**

Add this test in `src/compiler.rs` inside the `#[cfg(test)]` module (around line 5331, after other span tests):

```rust
#[test]
fn test_call_span_covers_callee_name() {
    // "local f = function(n) n; f(1)"
    //  0123456789012345678901234567890
    // 'f' at position 25, '(' at 26, ')' at 28 → span should be 25..29
    let input = "local f = function(n) n; f(1)";
    let mut scanner = Scanner::new(input, "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    let call_pos = chunk
        .code
        .iter()
        .position(|&x| x == Opcode::Call as u8)
        .expect("Call opcode not found");

    let call_span = chunk.get_span(call_pos).unwrap();
    assert_eq!(
        call_span.start, 25,
        "call span should start at 'f' (25), not '(' (26)"
    );
    assert_eq!(
        call_span.end, 29,
        "call span should end after ')'"
    );
}
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
bazel test //:compiler_test --test_output=streamed 2>&1 | grep -A 5 "test_call_span_covers_callee_name"
```

Expected: FAILED — `call span should start at 'f' (25), not '(' (26)` (the current start is 26).

- [ ] **Step 3: Add `lhs_span_start` field to the `Compiler` struct**

In `src/compiler.rs` at the `Compiler` struct (lines 172–187), add the field after `anon_stack_depth`:

Change:
```rust
    anon_stack_depth: usize, // Count of anonymous temporaries on VM stack not tracked as locals
}
```

To:
```rust
    anon_stack_depth: usize, // Count of anonymous temporaries on VM stack not tracked as locals
    lhs_span_start: usize,   // Start byte of the callee expression for call span attribution
}
```

In the `Compiler::new` initializer (lines 194–209), add after `anon_stack_depth: 0`:

Change:
```rust
            anon_stack_depth: 0,
        }
```

To:
```rust
            anon_stack_depth: 0,
            lhs_span_start: 0,
        }
```

- [ ] **Step 4: Set and restore `lhs_span_start` in `parse_expr`**

In `src/compiler.rs` at `parse_expr` (lines 260–303), update the function body:

Change:
```rust
    fn parse_expr(
        &mut self,
        min_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        // Parse left-hand side (prefix)
        self.parse_prefix(memory_manager)?;
```

To:
```rust
    fn parse_expr(
        &mut self,
        min_bp: u8,
        memory_manager: &mut MemoryManager,
    ) -> Result<(), CompilerError> {
        // Record where this expression starts for call span attribution (see parse_infix LeftParen).
        // Must be saved before and restored after parse_prefix because recursive parse_expr calls
        // inside parse_prefix (e.g. for nested expressions) overwrite the field.
        let expr_start = self.parser.current_token().map(|t| t.span.start).unwrap_or(0);
        self.lhs_span_start = expr_start;

        // Parse left-hand side (prefix)
        self.parse_prefix(memory_manager)?;

        // Restore: parse_prefix may have made recursive parse_expr calls that overwrote lhs_span_start.
        self.lhs_span_start = expr_start;
```

- [ ] **Step 5: Use `lhs_span_start` in `parse_infix` for `Token::LeftParen`**

In `src/compiler.rs` at line 1499, change:

```rust
            Token::LeftParen => {
                let call_start = token.span.start; // byte position of '(' for span attribution
```

To:

```rust
            Token::LeftParen => {
                let call_start = self.lhs_span_start; // start of the callee expression
```

- [ ] **Step 6: Run the test to confirm it passes**

```bash
bazel test //:compiler_test --test_output=streamed 2>&1 | grep -A 5 "test_call_span_covers_callee_name"
```

Expected: PASSED

- [ ] **Step 7: Run the full test suite**

```bash
bazel test //... 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 8: Verify end-to-end rendering**

```bash
bazel run //:main_stress -- /home/scott/Projects/RapidJsonnet/end2end/error_stack_render.jsonnet 2>&1 | grep -A 30 "Runtime error"
```

Expected: call-site labels now show the full call expression — `firstFunction()` instead of `()` for the outer frame, `upperFunction() + 2` or similar for the middle frame (the `+` is part of the expression), `x(2)` for the innermost.

- [ ] **Step 9: Format check**

```bash
bazel build --config=rustfmt //... 2>&1 | tail -5
```

If it fails, run:

```bash
bazel run @rules_rust//:rustfmt
```

Then re-run step 7 to confirm tests still pass.

- [ ] **Step 10: Commit**

```bash
git add src/compiler.rs
git commit -m "fix: call span covers callee name, not just argument list"
```

---

### Task 2: Suppress spurious thunk frame in stack traces

**Files:**
- Modify: `src/virtual_machine.rs:1256-1259` — skip `call_site` assignment for thunk closures in `call_closure`

**Background:** When `value_to_json` forces a field thunk (e.g. `should_fail: f()`), it calls `execute_thunk_sync_with_field` → `call_closure`. Inside `call_closure`, `call_site` is set using `get_current_span()` — but `instruction_start_ip` is stale from the previous `interpret()` cycle (pointing to the top-level script's Return instruction, near the end of the file). This stale span becomes the outermost entry in the stack trace. The fix: check `closure.is_thunk` before setting `call_site`. Thunks are lazy-evaluation implementation details, not user-written function calls — they have no meaningful call site.

- [ ] **Step 1: Write a failing test that asserts no thunk frame appears in the stack trace**

Add this test in `src/virtual_machine.rs` inside the `#[cfg(test)]` module, after `test_stack_trace_depth` (around line 17031):

```rust
#[test]
fn test_stack_trace_no_spurious_thunk_frame() {
    // A field value that errors should NOT produce an extra thunk frame.
    // Chain should be: root error + one call-site frame for f() = depth 2.
    // Before the fix, the stale thunk frame pushed depth to 3.
    let source = "local f = function() error 'boom'; { should_fail: f() }";
    let mut scanner_inst = scanner::Scanner::new(source, "test.jsonnet");
    let mut memory_manager = MemoryManager::new();
    let compiler_inst = compiler::Compiler::new(&mut scanner_inst, "test.jsonnet");
    let chunk = compiler_inst.compile(&mut memory_manager).expect("compile");
    let err = execute_with_ext_vars(chunk, memory_manager, &[], &[], &[])
        .expect_err("expected runtime error");

    let mut depth = 0;
    let mut current = &err;
    loop {
        depth += 1;
        match &current.cause {
            Some(next) => current = next,
            None => break,
        }
    }
    assert_eq!(
        depth, 2,
        "expected 2 entries in cause chain (error + f() call site), got {}",
        depth
    );
}
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
bazel test //:virtual_machine_test --test_output=streamed 2>&1 | grep -A 5 "test_stack_trace_no_spurious_thunk_frame"
```

Expected: FAILED — `expected 2 entries in cause chain … got 3`

- [ ] **Step 3: Skip `call_site` assignment for thunk closures in `call_closure`**

In `src/virtual_machine.rs` at lines 1256–1259, change:

```rust
        new_frame.call_site = Some((
            self.get_current_span(),
            self.current_chunk().source_id.to_string(),
        ));
```

To:

```rust
        // Thunks are lazy-evaluation internals; their "call site" is stale (instruction_start_ip
        // from the previous interpret cycle). Only record call_site for genuine function calls.
        if !closure.is_thunk {
            new_frame.call_site = Some((
                self.get_current_span(),
                self.current_chunk().source_id.to_string(),
            ));
        }
```

- [ ] **Step 4: Run the test to confirm it passes**

```bash
bazel test //:virtual_machine_test --test_output=streamed 2>&1 | grep -A 5 "test_stack_trace_no_spurious_thunk_frame"
```

Expected: PASSED

- [ ] **Step 5: Run the full test suite**

```bash
bazel test //... 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 6: Verify end-to-end rendering shows no spurious final label**

```bash
bazel run //:main_stress -- /home/scott/Projects/RapidJsonnet/end2end/error_stack_render.jsonnet 2>&1 | grep -A 30 "Runtime error"
```

Expected: only three call-site labels (innermost-first: `x(2)` at line 7, `upperFunction()` at line 9, `firstFunction()` at line 15). No extra label pointing to the end of the file.

- [ ] **Step 7: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "fix: suppress spurious thunk frame in stack traces"
```
