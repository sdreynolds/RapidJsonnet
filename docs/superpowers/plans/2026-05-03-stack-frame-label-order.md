# Stack Frame Label Order Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reverse the order call-site labels are added to the ariadne report so they render innermost-to-outermost (standard stack trace order) instead of outermost-to-innermost.

**Architecture:** Ariadne renders labels in addition order when spans are in separate source sections. `ScanError::into_report` adds labels head-to-root (outermost first), producing the reversed visual. Changing `.take(chain.len().saturating_sub(1))` to `.rev().skip(1)` on the chain iterator reverses the addition order. No API changes, no caller changes.

**Tech Stack:** Rust, ariadne 0.5.0, Bazel/rules_rust

---

### Task 1: Fix label order in `into_report`

**Files:**
- Modify: `src/scanner.rs:132-139` — labels loop in `into_report`
- Modify: `src/scanner.rs` — update existing `test_into_report_with_cause` to assert label order

- [ ] **Step 1: Write a failing test that asserts rendered label order**

Replace the existing `test_into_report_with_cause` in `src/scanner.rs` (around line 1067) with the following. The test renders the report to a `Vec<u8>` and asserts "inner frame" appears before "outer frame" in the output. Spans are placed on different lines (separated by blank lines) so ariadne puts each in its own source section; sections render in addition order.

```rust
#[test]
fn test_into_report_with_cause() {
    // Source with three spans on well-separated lines so ariadne
    // places each in its own section (sections render in addition order).
    // Line 1: "abc" (chars 0..3)   ← root error span
    // Line 4: "def" (chars 6..9)   ← inner call site span
    // Line 7: "ghi" (chars 12..15) ← outer call site span
    let source_text = "abc\n\n\ndef\n\n\nghi";

    let root = ScanError::new(0..3, "root error".to_string(), "test".to_string());
    let mut inner = ScanError::new(6..9, "inner frame".to_string(), "test".to_string());
    inner.cause = Some(Box::new(root));
    let mut outer = ScanError::new(12..15, "outer frame".to_string(), "test".to_string());
    outer.cause = Some(Box::new(inner));

    let (report, source_ids) = outer.into_report();
    assert_eq!(source_ids.len(), 1);

    // Render to bytes — ariadne::Report::write accepts any W: Write
    let mut output = Vec::<u8>::new();
    report
        .write(
            ("test".to_string(), ariadne::Source::from(source_text)),
            &mut output,
        )
        .unwrap();
    let rendered = String::from_utf8_lossy(&output).into_owned();

    let inner_pos = rendered.find("inner frame").expect("inner frame label missing");
    let outer_pos = rendered.find("outer frame").expect("outer frame label missing");
    assert!(
        inner_pos < outer_pos,
        "expected innermost label before outermost in rendered output\n{}",
        rendered
    );
}
```

- [ ] **Step 2: Run the test to confirm it fails**

```bash
bazel test //:scanner_test --test_output=streamed 2>&1 | grep -A 10 "test_into_report_with_cause"
```

Expected: FAILED — `expected innermost label before outermost in rendered output`

- [ ] **Step 3: Change the labels loop in `into_report`**

In `src/scanner.rs` at line 132, change the labels loop from:

```rust
        // Add caller frames as additional labels (all except the last/root cause)
        for frame in chain.iter().take(chain.len().saturating_sub(1)) {
            builder = builder.with_label(
                Label::new((frame.source_id.clone(), frame.span.clone()))
                    .with_message(&frame.message)
                    .with_color(yellow),
            );
        }
```

To:

```rust
        // Add caller frames as additional labels, innermost first so ariadne
        // renders them closest-to-error first (standard stack trace order).
        for frame in chain.iter().rev().skip(1) {
            builder = builder.with_label(
                Label::new((frame.source_id.clone(), frame.span.clone()))
                    .with_message(&frame.message)
                    .with_color(yellow),
            );
        }
```

- [ ] **Step 4: Run the test to confirm it passes**

```bash
bazel test //:scanner_test --test_output=streamed 2>&1 | grep -A 5 "test_into_report_with_cause"
```

Expected: PASSED

- [ ] **Step 5: Run the full test suite**

```bash
bazel test //... 2>&1 | tail -5
```

Expected: all tests pass.

- [ ] **Step 6: Verify end-to-end rendering shows correct order**

```bash
bazel run //:main_stress -- /home/scott/Projects/RapidJsonnet/end2end/error_stack_render.jsonnet 2>&1 | grep -A 30 "Runtime error"
```

Expected: call-site sections appear in ascending line order after the primary error —
`x(2)` at line 7 first, then `upperFunction() + 2` at line 11, then `firstFunction()` at line 15.

- [ ] **Step 7: Commit**

```bash
git add src/scanner.rs
git commit -m "fix: render stack frame labels innermost-first in error reports"
```
