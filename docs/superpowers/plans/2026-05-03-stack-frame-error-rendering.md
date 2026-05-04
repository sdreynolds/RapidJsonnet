# Stack Frame Error Rendering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich runtime error messages with a full call-site trace so users can see the chain of function calls that led to the error.

**Architecture:** Add a `call_site` field to `CallFrame` that captures the span of the `Call` opcode in the parent frame at push time. When `interpret()` receives a `RuntimeError`, walk the live frame stack and wrap the error in a `ScanError` cause chain — one entry per call site — so the existing `into_report` renderer emits bare source labels for each frame.

**Tech Stack:** Rust, ariadne (error rendering), existing `ScanError` cause chain in `src/scanner.rs`

---

### Task 1: Add `call_site` field to `CallFrame`

**Files:**
- Modify: `src/virtual_machine.rs:35-71`

- [ ] **Step 1: Write a failing test**

Add this test inside the `#[cfg(test)]` module near the bottom of `src/virtual_machine.rs` (alongside the existing tests):

```rust
#[test]
fn test_stack_trace_single_call() {
    // call site should appear in cause chain when a runtime error bubbles up
    let source = "local f = function() error 'boom'; f()";
    let err = run_jsonnet(source).expect_err("expected runtime error");
    // The error itself has no cause (single frame), but call_site from f's frame
    // should have been captured. With one level of wrapping there IS a cause.
    assert!(
        err.cause.is_some(),
        "expected cause chain for a nested call, got none"
    );
}
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
bazel test //:virtual_machine_test --test_output=streamed 2>&1 | grep -A 5 "test_stack_trace_single_call"
```

Expected: FAILED — `assertion failed: expected cause chain for a nested call, got none`

- [ ] **Step 3: Add `call_site` to `CallFrame`**

In `src/virtual_machine.rs`, add the field to the struct (after `cache_target`):

```rust
pub struct CallFrame {
    pub closure: ClosureIndex,
    pub ip: usize,
    pub stack_base: usize,
    pub self_obj: Option<ObjectIndex>,
    pub super_obj: Option<ObjectIndex>,
    pub field_name: Option<StringIndex>,
    pub cache_target: Option<(ObjectIndex, StringIndex, ObjectIndex)>,
    /// Byte span and source_id of the Call opcode in the parent frame.
    /// None for the top-level script frame (no parent).
    pub call_site: Option<(Range<usize>, String)>,
}
```

- [ ] **Step 4: Initialize `call_site` to `None` in `CallFrame::new`**

Update the `Self { ... }` block in `CallFrame::new`:

```rust
Self {
    closure,
    ip,
    stack_base,
    self_obj,
    super_obj,
    field_name: None,
    cache_target: None,
    call_site: None,
}
```

- [ ] **Step 5: Verify it compiles**

```bash
bazel build //... 2>&1 | grep -E "error|warning" | head -20
```

Expected: build succeeds (test still fails — that's fine, we haven't wired call_site up yet).

---

### Task 2: Populate `call_site` in `call_closure`

**Files:**
- Modify: `src/virtual_machine.rs` — `call_closure` function (around line 1247)

- [ ] **Step 1: Capture the call site before pushing the new frame**

In `call_closure`, after `new_frame.cache_target = self.pending_cache_target.take();` and before the frame push block, add:

```rust
new_frame.call_site = Some((
    self.get_current_span(),
    self.current_chunk().source_id.to_string(),
));
```

The full block after this change looks like:

```rust
let mut new_frame = CallFrame::new(closure_index, 0, stack_base, self_obj, super_obj);

new_frame.field_name = self.pending_field_name.take();
new_frame.cache_target = self.pending_cache_target.take();
new_frame.call_site = Some((
    self.get_current_span(),
    self.current_chunk().source_id.to_string(),
));

// Push frame
if self.frame_count < self.frames.len() {
    self.frames[self.frame_count] = new_frame;
} else {
    self.frames.push(new_frame);
}
self.frame_count += 1;
```

- [ ] **Step 2: Verify it compiles**

```bash
bazel build //... 2>&1 | grep -E "^.*error" | head -10
```

Expected: build succeeds.

---

### Task 3: Add `build_stack_trace` and wire into `interpret`

**Files:**
- Modify: `src/virtual_machine.rs` — add method, update `interpret`

- [ ] **Step 1: Add the `build_stack_trace` method**

Add this as a private method on `VirtualMachine`, just before `pub fn interpret`:

```rust
fn build_stack_trace(&self, error: RuntimeError) -> RuntimeError {
    let mut err = error;
    for frame in self.frames[1..self.frame_count].iter().rev() {
        if let Some((span, source_id)) = &frame.call_site {
            let mut wrapper = RuntimeError::new(span.clone(), String::new(), source_id.clone());
            wrapper.cause = Some(Box::new(err));
            err = wrapper;
        }
    }
    err
}
```

- [ ] **Step 2: Update `interpret` to call `build_stack_trace`**

Change `interpret` from:

```rust
pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
    self.interpret_until(0)
}
```

to:

```rust
pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
    self.interpret_until(0).map_err(|e| self.build_stack_trace(e))
}
```

- [ ] **Step 3: Run the failing test — it should now pass**

```bash
bazel test //:virtual_machine_test --test_output=streamed 2>&1 | grep -A 5 "test_stack_trace_single_call"
```

Expected: PASSED

- [ ] **Step 4: Run the full test suite to check for regressions**

```bash
bazel test //... 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "feat: add call-site stack trace to runtime error reports"
```

---

### Task 4: Add a deeper stack trace test and verify end-to-end rendering

**Files:**
- Modify: `src/virtual_machine.rs` — add test
- Read: `end2end/error_stack_render.jsonnet` (already updated, no changes needed)

- [ ] **Step 1: Add a three-level cause chain test**

Add inside the `#[cfg(test)]` module:

```rust
#[test]
fn test_stack_trace_depth() {
    // three levels deep: top-level → f → g → error
    let source = r#"
        local g = function() error "boom";
        local f = function() g();
        f()
    "#;
    let err = run_jsonnet(source).expect_err("expected runtime error");

    // Collect the full cause chain
    let mut chain_depth = 0;
    let mut current = &err;
    loop {
        chain_depth += 1;
        match &current.cause {
            Some(next) => current = next,
            None => break,
        }
    }
    // Root error (g's frame) + call site for g() in f + call site for f() at top
    assert_eq!(chain_depth, 3, "expected 3 entries in cause chain");
}
```

- [ ] **Step 2: Run the new test**

```bash
bazel test //:virtual_machine_test --test_output=streamed 2>&1 | grep -A 5 "test_stack_trace_depth"
```

Expected: PASSED

- [ ] **Step 3: Verify end-to-end rendering manually**

```bash
bazel run //:main_stress -- /home/scott/Projects/RapidJsonnet/end2end/error_stack_render.jsonnet 2>&1 | grep -v "^INFO\|^Computing\|^Loading\|^Analyzing\|^Target\|^Elapsed\|^Running"
```

Expected output shows the primary error at line 3 plus bare source labels at line 7 (`x(2)`), line 11 (`upperFunction() + 2`), and line 15 (`firstFunction()`). Example:

```
❌ Runtime error during execution:
Error: Must concatenate objects with other objects
   ╭─[ error_stack_render.jsonnet:3:5 ]
   ...
   ╭─[ error_stack_render.jsonnet:7:3 ]
   ...
   ╭─[ error_stack_render.jsonnet:11:3 ]
   ...
   ╭─[ error_stack_render.jsonnet:15:... ]
   ...
```

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "test: add stack trace depth and cause chain tests"
```
