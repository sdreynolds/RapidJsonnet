# Stack Frame Error Rendering

**Date:** 2026-05-03  
**Status:** Approved

## Problem

When a runtime error occurs inside a nested function call, the error report shows only the failure site. There is no call chain, so the user cannot tell which sequence of calls led to the error.

**Current output** for `error_stack_render.jsonnet`:
```
Error: Must concatenate objects with other objects
   ╭─[ error_stack_render.jsonnet:3:5 ]
   │
 3 │   n + y
   │     ┬  
   │     ╰── Must concatenate objects with other objects
───╯
```

**Expected output** — primary error at the failure site, plus bare call-site labels at each frame in the call chain:
```
Error: Must concatenate objects with other objects
   ╭─[ error_stack_render.jsonnet:3:5 ]    ← error inside x
   ...
   ╭─[ error_stack_render.jsonnet:7:3 ]    ← x(2) in upperFunction
   ╭─[ error_stack_render.jsonnet:11:3 ]   ← upperFunction() in firstFunction
   ╭─[ error_stack_render.jsonnet:15:16 ]  ← firstFunction() at top level
```

## Scope

Runtime errors only. Compile-time errors are out of scope.

## Design

### Section 1: Data Model

Add one field to `CallFrame` in `src/virtual_machine.rs`:

```rust
pub call_site: Option<(Range<usize>, String)>,
```

- `Range<usize>` — byte span of the `Call` opcode in the parent frame's chunk
- `String` — `source_id` of the parent frame's chunk

`frames[0]` (the top-level script frame) always has `None` because it has no parent. All other frames are populated at push time.

`CallFrame::new` initializes the field to `None`. The call site is set in `call_closure` immediately before pushing the new frame:

```rust
new_frame.call_site = Some((self.get_current_span(), self.current_chunk().source_id.to_string()));
```

At that point `instruction_start_ip` still holds the IP of the `Call` opcode — so `get_current_span()` returns the exact call-site span.

### Section 2: Error Enrichment

Add a private method to `VirtualMachine`:

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

Walking in reverse (innermost to outermost) builds the `ScanError` cause chain so the outermost call site is the head. `into_report` traverses the chain, uses the tail (the original error) as the primary ariadne report, and renders each call-site span as a bare yellow label with no annotation text.

Update `interpret()` to call this:

```rust
pub fn interpret(&mut self) -> Result<Value, RuntimeError> {
    self.interpret_until(0).map_err(|e| self.build_stack_trace(e))
}
```

No changes to `ScanError`, `into_report`, or ariadne rendering are needed.

### Section 3: Edge Cases

| Case | Behavior |
|------|----------|
| Thunk frames (`force_thunk`, `execute_thunk_sync`) | Also go through `call_closure`, so they get call-site spans. They point to the expression that forced evaluation — useful context. |
| `interpret_until` with non-zero `target_frame_count` | `build_stack_trace` uses `self.frame_count` at error time, which reflects only active frames. |
| Empty label message | `ScanError` allows an empty `message`. Ariadne renders a bare label arrow with no text — the source line itself provides context. |
| All frames included | `frames[1..frame_count]` includes every frame from the first user call through the innermost, including the top-level invocation. |

## Test File

`end2end/error_stack_render.jsonnet` — already updated to produce a runtime type error:

```jsonnet
local y = {some_object: "yep"};
local x = function(n) (
  n + y
);

local upperFunction = function() (
  x(2)
);

local firstFunction = function() (
  upperFunction() + 2
);

{
  should_fail: firstFunction()
}
```

`n + y` (number + object) fails at runtime with "Must concatenate objects with other objects". The expected call chain in the report: error at line 3, call sites at lines 7, 11, and 15.

## Files Changed

| File | Change |
|------|--------|
| `src/virtual_machine.rs` | Add `call_site` field to `CallFrame`; populate in `call_closure`; add `build_stack_trace`; update `interpret` |
| `end2end/error_stack_render.jsonnet` | Already updated (runtime error) |
