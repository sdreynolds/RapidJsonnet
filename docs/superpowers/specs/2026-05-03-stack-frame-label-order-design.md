# Stack Frame Label Order Design

**Date:** 2026-05-03  
**Status:** Approved

## Problem

The current stack frame rendering in ariadne shows call-site labels in outermost-to-innermost order (line 15 → line 11 → line 7), which is the reverse of the standard stack trace convention. The standard reading order is: primary error first, then call sites from innermost (closest to the error) to outermost (entry point).

**Current output:**
```
Error: Must concatenate objects with other objects
    ╭─[ error_stack_render.jsonnet:3:5 ]
  3 │   n + y                              ← error (primary, first)
 16 │ }                                    ← thunk frame (outermost, shown first)
 15 │   should_fail: firstFunction()
 11 │   upperFunction() + 2
  7 │   x(2)                               ← innermost call site (shown last)
────╯
```

**Expected output:**
```
Error: Must concatenate objects with other objects
    ╭─[ error_stack_render.jsonnet:3:5 ]
  3 │   n + y                              ← error (primary, first — fine)
  7 │   x(2)                              ← innermost call site (shown first after error)
 11 │   upperFunction() + 2
 15 │   should_fail: firstFunction()      ← outermost (shown last)
────╯
```

## Root Cause

Ariadne renders labels in the order they are added to the `Report`. `ScanError::into_report` currently iterates the cause chain from head (outermost call site) to the entry before root, adding labels outermost-first. This produces the reversed visual order.

## Design

### Single Change: Reverse the labels loop in `into_report`

**File:** `src/scanner.rs` — `ScanError::into_report`, around line 133

**Current:**
```rust
for frame in chain.iter().take(chain.len().saturating_sub(1)) {
    builder = builder.with_label(
        Label::new((frame.source_id.clone(), frame.span.clone()))
            .with_message(&frame.message)
            .with_color(yellow),
    );
}
```

**After:**
```rust
for frame in chain.iter().rev().skip(1) {
    builder = builder.with_label(
        Label::new((frame.source_id.clone(), frame.span.clone()))
            .with_message(&frame.message)
            .with_color(yellow),
    );
}
```

`chain.iter().rev().skip(1)` skips the root (already used as the primary span) and walks from innermost call site to outermost. Ariadne then renders labels in that addition order.

### Why this works

Ariadne renders labels in the order they are added to the `Report` builder, not sorted by source position. The primary span (error at line 3) is always the anchor and appears first. By adding innermost call site first, that frame appears immediately after the primary error, giving the standard stack trace reading direction.

### Scope

- No API changes to `into_report`'s signature
- No changes to callers (`main.rs`, REPL, test runner)
- Compile error cause chains also benefit — context labels render in natural reading order

## Files Changed

| File | Change |
|------|--------|
| `src/scanner.rs` | Change `.take(chain.len().saturating_sub(1))` to `.rev().skip(1)` in the labels loop |
