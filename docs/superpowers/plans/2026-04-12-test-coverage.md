# Test Coverage Improvement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Achieve ≥85% line coverage across `native.rs`, `virtual_machine.rs`, `compiler.rs`, `chunk.rs`, and `scanner.rs`.

**Architecture:** Each file is tackled in priority order (worst coverage first). Happy-path tests use the `Scanner → Compiler → VirtualMachine::interpret()` pipeline inside `#[cfg(test)]` blocks (these count toward coverage; shell-based end2end tests do not). Error-path tests call internal APIs directly. `native.rs` pipeline tests live in `virtual_machine.rs` because only that crate depends on both the compiler and native libraries.

**Tech Stack:** Rust, Bazel/rules_rust, `bazel test`, `bazel coverage`

---

## File Map

| File | Action | Purpose |
|------|--------|---------|
| `BUILD.bazel` | Modify | Add `native_test` rust_test target |
| `src/native.rs` | Modify | Add `#[cfg(test)]` mod with direct `call_native` unit tests |
| `src/virtual_machine.rs` | Modify | Expand `mod tests` with pipeline tests for all std functions + language features |
| `src/compiler_integration_test.rs` | Modify | Add pipeline tests for uncovered compile paths |
| `src/compiler.rs` | Modify | Add error-path unit tests |
| `src/chunk.rs` | Modify | Expand `mod tests` for disassembler and large-operand paths |
| `src/scanner.rs` | Modify | Expand `mod tests` for text blocks, verbatim strings, unicode escapes |

---

## Shared Helper: run_jsonnet

All pipeline tests in `virtual_machine.rs` use this helper (add it once at the top of `mod tests` if not already present):

```rust
fn run_jsonnet(source: &str) -> Result<Value, RuntimeError> {
    let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
    let mut memory_manager = MemoryManager::new();
    let compiler = compiler::Compiler::new(&mut scanner, "test.jsonnet");
    let chunk = compiler.compile(&mut memory_manager).expect("compile failed");
    let mut vm = VirtualMachine::new(chunk, memory_manager);
    vm.interpret()
}

fn assert_bool(source: &str) {
    let result = run_jsonnet(source).expect("expected success");
    assert_eq!(result, Value::Boolean(true), "source: {}", source);
}
```

---

## Task 1: Add `native_test` BUILD Target

**Files:**
- Modify: `BUILD.bazel`

- [ ] **Step 1: Add the test target**

In `BUILD.bazel`, after the existing `memory_manager_test` entry, add:

```python
rust_test(
    name = "native_test",
    crate = ":native",
)
```

- [ ] **Step 2: Verify it builds (no tests yet)**

```bash
bazel test //:native_test
```

Expected: PASS (0 tests run — no `#[cfg(test)]` block exists yet).

- [ ] **Step 3: Commit**

```bash
git add BUILD.bazel
git commit -m "build: add native_test rust_test target"
```

---

## Task 2: native.rs — Math Function Error Paths

**Files:**
- Modify: `src/native.rs` (add `#[cfg(test)]` module)

- [ ] **Step 1: Add the test module skeleton and first tests**

Append to `src/native.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use memory_manager::MemoryManager;

    fn span() -> std::ops::Range<usize> {
        0..1
    }

    fn sid() -> String {
        "test".to_string()
    }

    fn mk_string(mm: &mut MemoryManager, s: &str) -> Value {
        Value::String(mm.allocate_string(s).index)
    }

    fn mk_array(mm: &mut MemoryManager, elems: Vec<Value>) -> Value {
        Value::Array(mm.allocate_array(elems).index)
    }

    #[test]
    fn test_abs_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello");
        let err = call_native(NativeFuncId::Abs, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_abs_arity_mismatch() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Abs, &[], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("expected"), "got: {}", err.message);
    }

    #[test]
    fn test_floor_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "x");
        let err = call_native(NativeFuncId::Floor, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_ceil_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Ceil, &[Value::Boolean(true)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_round_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Round, &[Value::Null], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_sign_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "x");
        let err = call_native(NativeFuncId::Sign, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_sqrt_negative() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Sqrt, &[Value::Number(-1.0)], &mut mm, span(), sid());
        // Should either error or return NaN depending on implementation
        match result {
            Ok(Value::Number(n)) => assert!(n.is_nan()),
            Err(_) => {} // also acceptable
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn test_pow_wrong_types() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "x");
        let err = call_native(NativeFuncId::Pow, &[s, Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_clamp_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "x");
        let err = call_native(NativeFuncId::Clamp, &[s, Value::Number(0.0), Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }
}
```

- [ ] **Step 2: Run and verify tests pass**

```bash
bazel test //:native_test
```

Expected: PASS (all new tests pass since error handling already exists).

- [ ] **Step 3: Commit**

```bash
git add src/native.rs
git commit -m "test: add native.rs math error-path unit tests"
```

---

## Task 3: native.rs — String Function Error Paths

**Files:**
- Modify: `src/native.rs`

- [ ] **Step 1: Add string error-path tests inside the existing `mod tests` block**

```rust
    #[test]
    fn test_codepoint_multi_char() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "ab");
        let err = call_native(NativeFuncId::Codepoint, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("single"), "got: {}", err.message);
    }

    #[test]
    fn test_codepoint_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Codepoint, &[Value::Number(65.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("string"), "got: {}", err.message);
    }

    #[test]
    fn test_char_out_of_range() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Char, &[Value::Number(0x110000_f64)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_char_surrogate() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Char, &[Value::Number(0xD800_f64)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_char_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "A");
        let err = call_native(NativeFuncId::Char, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("number"), "got: {}", err.message);
    }

    #[test]
    fn test_substr_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Substr, &[Value::Number(1.0), Value::Number(0.0), Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("string"), "got: {}", err.message);
    }

    #[test]
    fn test_parse_int_invalid() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "xyz");
        let err = call_native(NativeFuncId::ParseInt, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_parse_octal_invalid() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "9");
        let err = call_native(NativeFuncId::ParseOctal, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_parse_hex_invalid() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "gg");
        let err = call_native(NativeFuncId::ParseHex, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_length_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Length, &[Value::Number(5.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("length"), "got: {}", err.message);
    }

    #[test]
    fn test_starts_with_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "foo");
        let err = call_native(NativeFuncId::StartsWith, &[Value::Number(1.0), s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_ends_with_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "foo");
        let err = call_native(NativeFuncId::EndsWith, &[Value::Number(1.0), s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_split_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Split, &[Value::Number(1.0), Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_ascii_upper_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::AsciiUpper, &[Value::Boolean(true)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:native_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/native.rs
git commit -m "test: add native.rs string error-path unit tests"
```

---

## Task 4: native.rs — Array and Object Error Paths

**Files:**
- Modify: `src/native.rs`

- [ ] **Step 1: Add array and object error-path tests**

```rust
    #[test]
    fn test_flatten_arrays_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::FlattenArrays, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_reverse_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello");
        let err = call_native(NativeFuncId::Reverse, &[s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_range_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "a");
        let err = call_native(NativeFuncId::Range, &[s, Value::Number(5.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_object_fields_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::ObjectFields, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_object_has_wrong_type() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "key");
        let err = call_native(NativeFuncId::ObjectHas, &[Value::Number(1.0), s], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_object_values_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::ObjectValues, &[Value::Boolean(false)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_member_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Member, &[Value::Number(1.0), Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_count_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Count, &[Value::Number(1.0), Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_sum_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Sum, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_find_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Find, &[Value::Number(1.0), Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_assert_equal_mismatch() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::AssertEqual, &[Value::Number(1.0), Value::Number(2.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(err.message.contains("assert"), "got: {}", err.message);
    }

    #[test]
    fn test_join_wrong_type() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0)]);
        let err = call_native(NativeFuncId::Join, &[Value::Number(1.0), arr], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_coerce_to_sorted_array_wrong_type() {
        let mut mm = MemoryManager::new();
        // Calling a set function with a non-array/non-string triggers coerce_to_sorted_array error
        // SetUnion expects two array/string args
        let err = call_native(NativeFuncId::SetMember, &[Value::Number(1.0), Value::Boolean(false)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:native_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/native.rs
git commit -m "test: add native.rs array/object error-path unit tests"
```

---

## Task 5: native.rs — Direct Happy-Path Unit Tests

**Files:**
- Modify: `src/native.rs`

These test `call_native` directly (no full pipeline needed) and cover the execution paths of each function.

- [ ] **Step 1: Add happy-path tests**

```rust
    #[test]
    fn test_abs_positive() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Abs, &[Value::Number(-5.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_abs_zero() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Abs, &[Value::Number(0.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_floor() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Floor, &[Value::Number(3.7)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_ceil() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Ceil, &[Value::Number(3.1)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(4.0));
    }

    #[test]
    fn test_round() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Round, &[Value::Number(3.5)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(4.0));
    }

    #[test]
    fn test_type_number() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Type, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "number");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_type_string() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hi");
        let result = call_native(NativeFuncId::Type, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "string");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_type_null() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Type, &[Value::Null], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "null");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_type_boolean() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Type, &[Value::Boolean(true)], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "boolean");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_type_array() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![]);
        let result = call_native(NativeFuncId::Type, &[arr], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "array");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_is_array_true() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![]);
        let result = call_native(NativeFuncId::IsArray, &[arr], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_array_false() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsArray, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_is_string_true() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hi");
        let result = call_native(NativeFuncId::IsString, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_number_true() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsNumber, &[Value::Number(3.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_boolean_true() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsBoolean, &[Value::Boolean(false)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_null_true() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsNull, &[Value::Null], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_length_string() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello");
        let result = call_native(NativeFuncId::Length, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_length_array() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0)]);
        let result = call_native(NativeFuncId::Length, &[arr], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn test_ascii_upper() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello");
        let result = call_native(NativeFuncId::AsciiUpper, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "HELLO");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_ascii_lower() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "WORLD");
        let result = call_native(NativeFuncId::AsciiLower, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "world");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_codepoint_a() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "A");
        let result = call_native(NativeFuncId::Codepoint, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(65.0));
    }

    #[test]
    fn test_char_65() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Char, &[Value::Number(65.0)], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "A");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_parse_int_valid() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "42");
        let result = call_native(NativeFuncId::ParseInt, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(42.0));
    }

    #[test]
    fn test_parse_hex_valid() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "ff");
        let result = call_native(NativeFuncId::ParseHex, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(255.0));
    }

    #[test]
    fn test_starts_with_true() {
        let mut mm = MemoryManager::new();
        let haystack = mk_string(&mut mm, "hello world");
        let needle = mk_string(&mut mm, "hello");
        let result = call_native(NativeFuncId::StartsWith, &[haystack, needle], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_ends_with_true() {
        let mut mm = MemoryManager::new();
        let haystack = mk_string(&mut mm, "hello world");
        let needle = mk_string(&mut mm, "world");
        let result = call_native(NativeFuncId::EndsWith, &[haystack, needle], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_empty_empty_string() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "");
        let result = call_native(NativeFuncId::IsEmpty, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_empty_nonempty_string() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "x");
        let result = call_native(NativeFuncId::IsEmpty, &[s], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_range_basic() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Range, &[Value::Number(0.0), Value::Number(3.0)], &mut mm, span(), sid()).unwrap();
        if let Value::Array(idx) = result {
            let arr = mm.load_array(idx);
            assert_eq!(arr.len(), 4);
        } else { panic!("expected array"); }
    }

    #[test]
    fn test_clamp_in_range() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Clamp, &[Value::Number(5.0), Value::Number(0.0), Value::Number(10.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(5.0));
    }

    #[test]
    fn test_clamp_below_min() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Clamp, &[Value::Number(-1.0), Value::Number(0.0), Value::Number(10.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_sum_empty() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![]);
        let result = call_native(NativeFuncId::Sum, &[arr], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(0.0));
    }

    #[test]
    fn test_sum_values() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
        let result = call_native(NativeFuncId::Sum, &[arr], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(6.0));
    }

    #[test]
    fn test_reverse_array() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
        let result = call_native(NativeFuncId::Reverse, &[arr], &mut mm, span(), sid()).unwrap();
        if let Value::Array(idx) = result {
            let arr = mm.load_array(idx);
            assert_eq!(arr.elements[0], Value::Number(3.0));
            assert_eq!(arr.elements[2], Value::Number(1.0));
        } else { panic!("expected array"); }
    }

    #[test]
    fn test_member_found() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0)]);
        let result = call_native(NativeFuncId::Member, &[arr, Value::Number(2.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_member_not_found() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0)]);
        let result = call_native(NativeFuncId::Member, &[arr, Value::Number(99.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_count_basic() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0), Value::Number(1.0)]);
        let result = call_native(NativeFuncId::Count, &[arr, Value::Number(1.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(2.0));
    }

    #[test]
    fn test_values_equal_numbers() {
        let mm = MemoryManager::new();
        assert!(values_equal(Value::Number(1.0), Value::Number(1.0), &mm));
        assert!(!values_equal(Value::Number(1.0), Value::Number(2.0), &mm));
    }

    #[test]
    fn test_values_equal_null() {
        let mm = MemoryManager::new();
        assert!(values_equal(Value::Null, Value::Null, &mm));
        assert!(!values_equal(Value::Null, Value::Boolean(false), &mm));
    }

    #[test]
    fn test_sign_positive() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Sign, &[Value::Number(5.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(1.0));
    }

    #[test]
    fn test_sign_negative() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Sign, &[Value::Number(-3.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(-1.0));
    }

    #[test]
    fn test_sign_zero() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Sign, &[Value::Number(0.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(0.0));
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:native_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/native.rs
git commit -m "test: add native.rs happy-path direct unit tests"
```

---

## Task 6: native.rs — Encoding, Format, and Remaining Functions

**Files:**
- Modify: `src/native.rs`

- [ ] **Step 1: Add encoding and format tests**

```rust
    #[test]
    fn test_base64_encode() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello");
        let result = call_native(NativeFuncId::Base64, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "aGVsbG8=");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_base64_wrong_type() {
        let mut mm = MemoryManager::new();
        let err = call_native(NativeFuncId::Base64, &[Value::Number(1.0)], &mut mm, span(), sid()).unwrap_err();
        assert!(!err.message.is_empty());
    }

    #[test]
    fn test_escape_string_json() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "say \"hi\"");
        let result = call_native(NativeFuncId::EscapeStringJson, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert!(mm.load_string(idx).contains("\\\""));
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_escape_string_bash() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello world");
        let result = call_native(NativeFuncId::EscapeStringBash, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            let out = mm.load_string(idx).to_string();
            assert!(out.contains('\''), "got: {}", out);
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_escape_string_xml() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "<tag>");
        let result = call_native(NativeFuncId::EscapeStringXml, &[s], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            let out = mm.load_string(idx).to_string();
            assert!(out.contains("&lt;"), "got: {}", out);
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_min_numbers() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Min, &[Value::Number(3.0), Value::Number(7.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_max_numbers() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::Max, &[Value::Number(3.0), Value::Number(7.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(7.0));
    }

    #[test]
    fn test_to_string_number() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::ToString, &[Value::Number(42.0)], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "42");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_is_even_true() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsEven, &[Value::Number(4.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_is_odd_true() {
        let mut mm = MemoryManager::new();
        let result = call_native(NativeFuncId::IsOdd, &[Value::Number(3.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_avg_basic() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(2.0), Value::Number(4.0)]);
        let result = call_native(NativeFuncId::Avg, &[arr], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Number(3.0));
    }

    #[test]
    fn test_contains_true() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0)]);
        let result = call_native(NativeFuncId::Contains, &[arr, Value::Number(1.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(true));
    }

    #[test]
    fn test_contains_false() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0)]);
        let result = call_native(NativeFuncId::Contains, &[arr, Value::Number(99.0)], &mut mm, span(), sid()).unwrap();
        assert_eq!(result, Value::Boolean(false));
    }

    #[test]
    fn test_find_substr_basic() {
        let mut mm = MemoryManager::new();
        let haystack = mk_string(&mut mm, "foobarfoo");
        let needle = mk_string(&mut mm, "foo");
        let result = call_native(NativeFuncId::FindSubstr, &[needle, haystack], &mut mm, span(), sid()).unwrap();
        if let Value::Array(idx) = result {
            let arr = mm.load_array(idx);
            assert_eq!(arr.len(), 2);
            assert_eq!(arr.elements[0], Value::Number(0.0));
            assert_eq!(arr.elements[1], Value::Number(6.0));
        } else { panic!("expected array"); }
    }

    #[test]
    fn test_str_replace_basic() {
        let mut mm = MemoryManager::new();
        let s = mk_string(&mut mm, "hello world");
        let from = mk_string(&mut mm, "world");
        let to = mk_string(&mut mm, "rust");
        let result = call_native(NativeFuncId::StrReplace, &[s, from, to], &mut mm, span(), sid()).unwrap();
        if let Value::String(idx) = result {
            assert_eq!(mm.load_string(idx), "hello rust");
        } else { panic!("expected string"); }
    }

    #[test]
    fn test_remove_basic() {
        let mut mm = MemoryManager::new();
        let arr = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0), Value::Number(3.0)]);
        let result = call_native(NativeFuncId::Remove, &[arr, Value::Number(2.0)], &mut mm, span(), sid()).unwrap();
        if let Value::Array(idx) = result {
            let arr = mm.load_array(idx);
            assert_eq!(arr.len(), 2);
        } else { panic!("expected array"); }
    }

    #[test]
    fn test_flatten_arrays_basic() {
        let mut mm = MemoryManager::new();
        let inner1 = mk_array(&mut mm, vec![Value::Number(1.0), Value::Number(2.0)]);
        let inner2 = mk_array(&mut mm, vec![Value::Number(3.0)]);
        let outer = mk_array(&mut mm, vec![inner1, inner2]);
        let result = call_native(NativeFuncId::FlattenArrays, &[outer], &mut mm, span(), sid()).unwrap();
        if let Value::Array(idx) = result {
            assert_eq!(mm.load_array(idx).len(), 3);
        } else { panic!("expected array"); }
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:native_test
```

Expected: PASS.

- [ ] **Step 3: Check coverage improvement**

```bash
bazel coverage //:native_test --combined_report=lcov
python3 -c "
import sys
content = open('$(bazel info output_path)/_coverage/_coverage_report.dat').read()
cur = None
hit = tot = 0
for line in content.split('\n'):
    if 'src/native.rs' in line and line.startswith('SF:'):
        cur = True
    elif cur and line.startswith('DA:'):
        p = line[3:].split(',')
        tot += 1
        if int(p[1]) > 0: hit += 1
    elif line == 'end_of_record':
        cur = False
print(f'native.rs: {hit/tot*100:.1f}% ({hit}/{tot})')
"
```

- [ ] **Step 4: Commit**

```bash
git add src/native.rs
git commit -m "test: add native.rs encoding/format/utility unit tests"
```

---

## Task 7: virtual_machine.rs — Native Pipeline Tests Part 1 (math, type, string)

**Files:**
- Modify: `src/virtual_machine.rs`

Add the `run_jsonnet`/`assert_bool` helpers (if not present) and pipeline tests for std functions. These cover both `native.rs` and `virtual_machine.rs` execution paths.

- [ ] **Step 1: Add helpers and math/type/string pipeline tests to `mod tests`**

Inside the existing `mod tests` block at the end of `src/virtual_machine.rs`, add:

```rust
    fn run_jsonnet(source: &str) -> Result<Value, RuntimeError> {
        let mut scanner_inst = scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = MemoryManager::new();
        let compiler_inst = compiler::Compiler::new(&mut scanner_inst, "test.jsonnet");
        let chunk = compiler_inst.compile(&mut memory_manager).expect("compile failed");
        let mut vm = VirtualMachine::new(chunk, memory_manager);
        vm.interpret()
    }

    fn assert_bool(source: &str) {
        match run_jsonnet(source).expect(&format!("expected success for: {}", source)) {
            Value::Boolean(true) => {}
            other => panic!("expected true, got {:?} for: {}", other, source),
        }
    }

    fn assert_err(source: &str, contains: &str) {
        let err = run_jsonnet(source).expect_err(&format!("expected error for: {}", source));
        assert!(err.message.contains(contains), "error '{}' did not contain '{}' for: {}", err.message, contains, source);
    }

    // --- std math ---

    #[test]
    fn test_std_abs_pipeline() {
        assert_bool("std.abs(-5) == 5");
        assert_bool("std.abs(3) == 3");
        assert_bool("std.abs(0) == 0");
    }

    #[test]
    fn test_std_floor_ceil_round_pipeline() {
        assert_bool("std.floor(3.7) == 3");
        assert_bool("std.ceil(3.2) == 4");
        assert_bool("std.round(3.5) == 4");
        assert_bool("std.round(3.4) == 3");
    }

    #[test]
    fn test_std_min_max_pipeline() {
        assert_bool("std.min(3, 7) == 3");
        assert_bool("std.max(3, 7) == 7");
    }

    #[test]
    fn test_std_sign_pipeline() {
        assert_bool("std.sign(5) == 1");
        assert_bool("std.sign(-3) == -1");
        assert_bool("std.sign(0) == 0");
    }

    #[test]
    fn test_std_clamp_pipeline() {
        assert_bool("std.clamp(5, 0, 10) == 5");
        assert_bool("std.clamp(-1, 0, 10) == 0");
        assert_bool("std.clamp(11, 0, 10) == 10");
    }

    #[test]
    fn test_std_pow_sqrt_pipeline() {
        assert_bool("std.pow(2, 10) == 1024");
        assert_bool("std.sqrt(9) == 3");
    }

    #[test]
    fn test_std_log_exp_pipeline() {
        assert_bool("std.log(1) == 0");
        assert_bool("std.exp(0) == 1");
    }

    #[test]
    fn test_std_trig_pipeline() {
        assert_bool("std.sin(0) == 0");
        assert_bool("std.cos(0) == 1");
        assert_bool("std.tan(0) == 0");
    }

    #[test]
    fn test_std_hypot_pipeline() {
        assert_bool("std.hypot(3, 4) == 5");
    }

    // --- std type predicates ---

    #[test]
    fn test_std_type_pipeline() {
        assert_bool(r#"std.type(1) == "number""#);
        assert_bool(r#"std.type("s") == "string""#);
        assert_bool(r#"std.type(null) == "null""#);
        assert_bool(r#"std.type(true) == "boolean""#);
        assert_bool(r#"std.type([]) == "array""#);
        assert_bool(r#"std.type({}) == "object""#);
        assert_bool(r#"std.type(function() 0) == "function""#);
    }

    #[test]
    fn test_std_is_predicates_pipeline() {
        assert_bool("std.isArray([])");
        assert_bool("std.isBoolean(true)");
        assert_bool("std.isNumber(1)");
        assert_bool("std.isObject({})");
        assert_bool("std.isString(\"hi\")");
        assert_bool("std.isNull(null)");
        assert_bool("std.isFunction(function() 0)");
        assert_bool("!std.isArray(1)");
    }

    // --- std string operations ---

    #[test]
    fn test_std_length_pipeline() {
        assert_bool(r#"std.length("hello") == 5"#);
        assert_bool("std.length([1,2,3]) == 3");
        assert_bool("std.length({a:1,b:2}) == 2");
    }

    #[test]
    fn test_std_substr_pipeline() {
        assert_bool(r#"std.substr("hello", 1, 3) == "ell""#);
        assert_bool(r#"std.substr("hello", 0, 5) == "hello""#);
    }

    #[test]
    fn test_std_split_join_pipeline() {
        assert_bool(r#"std.split("a,b,c", ",") == ["a","b","c"]"#);
        assert_bool(r#"std.join(",", ["a","b","c"]) == "a,b,c""#);
    }

    #[test]
    fn test_std_lines_pipeline() {
        assert_bool(r#"std.lines(["a","b"]) == "a\nb\n""#);
    }

    #[test]
    fn test_std_codepoint_char_pipeline() {
        assert_bool("std.codepoint(\"A\") == 65");
        assert_bool(r#"std.char(65) == "A""#);
    }

    #[test]
    fn test_std_to_string_pipeline() {
        assert_bool(r#"std.toString(42) == "42""#);
        assert_bool(r#"std.toString(true) == "true""#);
        assert_bool(r#"std.toString(null) == "null""#);
    }

    #[test]
    fn test_std_ascii_case_pipeline() {
        assert_bool(r#"std.asciiUpper("hello") == "HELLO""#);
        assert_bool(r#"std.asciiLower("WORLD") == "world""#);
    }

    #[test]
    fn test_std_starts_ends_with_pipeline() {
        assert_bool(r#"std.startsWith("hello", "he")"#);
        assert_bool(r#"std.endsWith("hello", "lo")"#);
        assert_bool(r#"!std.startsWith("hello", "lo")"#);
    }

    #[test]
    fn test_std_str_replace_pipeline() {
        assert_bool(r#"std.strReplace("hello world", "world", "rust") == "hello rust""#);
    }

    #[test]
    fn test_std_is_empty_pipeline() {
        assert_bool(r#"std.isEmpty("")"#);
        assert_bool(r#"!std.isEmpty("x")"#);
    }

    #[test]
    fn test_std_find_substr_pipeline() {
        assert_bool(r#"std.findSubstr("foo", "foobarfoo") == [0, 6]"#);
    }

    #[test]
    fn test_std_parse_pipeline() {
        assert_bool("std.parseInt(\"42\") == 42");
        assert_bool("std.parseHex(\"ff\") == 255");
        assert_bool("std.parseOctal(\"7\") == 7");
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:virtual_machine_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "test: add VM pipeline tests for std math/type/string functions"
```

---

## Task 8: virtual_machine.rs — Native Pipeline Tests Part 2 (arrays, objects, encoding)

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Add array, object, and encoding pipeline tests**

```rust
    // --- std array operations ---

    #[test]
    fn test_std_range_pipeline() {
        assert_bool("std.range(0, 3) == [0,1,2,3]");
    }

    #[test]
    fn test_std_reverse_pipeline() {
        assert_bool("std.reverse([1,2,3]) == [3,2,1]");
    }

    #[test]
    fn test_std_flatten_pipeline() {
        assert_bool("std.flattenArrays([[1,2],[3]]) == [1,2,3]");
    }

    #[test]
    fn test_std_sort_uniq_pipeline() {
        assert_bool("std.sort([3,1,2]) == [1,2,3]");
        assert_bool("std.uniq([1,1,2,2,3]) == [1,2,3]");
    }

    #[test]
    fn test_std_sum_avg_pipeline() {
        assert_bool("std.sum([1,2,3]) == 6");
        assert_bool("std.avg([2,4]) == 3");
    }

    #[test]
    fn test_std_member_contains_pipeline() {
        assert_bool("std.member([1,2,3], 2)");
        assert_bool("!std.member([1,2,3], 99)");
        assert_bool("std.contains([1,2,3], 2)");
    }

    #[test]
    fn test_std_count_find_pipeline() {
        assert_bool("std.count([1,2,1,3], 1) == 2");
        assert_bool("std.find(2, [1,2,3,2]) == [1,3]");
    }

    #[test]
    fn test_std_remove_pipeline() {
        assert_bool("std.remove([1,2,3], 2) == [1,3]");
    }

    #[test]
    fn test_std_all_any_pipeline() {
        assert_bool("std.all([true, true, true])");
        assert_bool("!std.all([true, false])");
        assert_bool("std.any([false, true])");
        assert_bool("!std.any([false, false])");
    }

    #[test]
    fn test_std_map_filter_pipeline() {
        assert_bool("std.map(function(x) x * 2, [1,2,3]) == [2,4,6]");
        assert_bool("std.filter(function(x) x > 1, [1,2,3]) == [2,3]");
    }

    #[test]
    fn test_std_foldl_foldr_pipeline() {
        assert_bool("std.foldl(function(acc, x) acc + x, [1,2,3], 0) == 6");
        assert_bool("std.foldr(function(x, acc) x + acc, [1,2,3], 0) == 6");
    }

    #[test]
    fn test_std_flatmap_pipeline() {
        assert_bool("std.flatMap(function(x) [x, x], [1,2]) == [1,1,2,2]");
    }

    #[test]
    fn test_std_make_array_pipeline() {
        assert_bool("std.makeArray(3, function(i) i * 2) == [0,2,4]");
    }

    #[test]
    fn test_std_string_chars_pipeline() {
        assert_bool(r#"std.stringChars("abc") == ["a","b","c"]"#);
    }

    // --- std object operations ---

    #[test]
    fn test_std_object_fields_pipeline() {
        assert_bool(r#"std.objectFields({a:1,b:2}) == ["a","b"]"#);
    }

    #[test]
    fn test_std_object_has_pipeline() {
        assert_bool(r#"std.objectHas({a:1}, "a")"#);
        assert_bool(r#"!std.objectHas({a:1}, "b")"#);
    }

    #[test]
    fn test_std_object_values_pipeline() {
        assert_bool("std.objectValues({a:1,b:2}) == [1,2]");
    }

    #[test]
    fn test_std_get_pipeline() {
        assert_bool(r#"std.get({a:1}, "a") == 1"#);
        assert_bool(r#"std.get({a:1}, "b", 0) == 0"#);
    }

    // --- std encoding ---

    #[test]
    fn test_std_base64_pipeline() {
        assert_bool(r#"std.base64("hello") == "aGVsbG8=""#);
    }

    #[test]
    fn test_std_escape_string_json_pipeline() {
        assert_bool(r#"std.escapeStringJson("say \"hi\"") == "\"say \\\"hi\\\"\"""#);
    }

    #[test]
    fn test_std_escape_string_xml_pipeline() {
        assert_bool(r#"std.escapeStringXml("<b>") == "&lt;b&gt;""#);
    }

    // --- std format ---

    #[test]
    fn test_std_format_string_pipeline() {
        assert_bool(r#"std.format("Hello %s", "world") == "Hello world""#);
        assert_bool(r#"std.format("%d", 42) == "42""#);
        assert_bool(r#"std.format("%.2f", 3.14159) == "3.14""#);
        assert_bool(r#"std.format("%05d", 42) == "00042""#);
        assert_bool(r#"std.format("100%%", []) == "100%""#);
        assert_bool(r#"std.format("%x", 255) == "ff""#);
        assert_bool(r#"std.format("%o", 8) == "10""#);
    }

    #[test]
    fn test_std_format_operator_pipeline() {
        assert_bool(r#""Hello %s" % "world" == "Hello world""#);
        assert_bool(r#""%d + %d" % [1, 2] == "1 + 2""#);
    }

    #[test]
    fn test_std_manifest_json_pipeline() {
        assert_bool(r#"std.manifestJson({a: 1}) != """#);
    }

    #[test]
    fn test_std_assert_equal_pipeline() {
        assert_bool("std.assertEqual(1, 1)");
    }

    #[test]
    fn test_std_deep_join_pipeline() {
        assert_bool(r#"std.deepJoin(["a", ["b", "c"]]) == "abc""#);
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:virtual_machine_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "test: add VM pipeline tests for std array/object/encoding/format functions"
```

---

## Task 9: virtual_machine.rs — Language Feature Pipeline Tests

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Add conditional and object tests**

```rust
    // --- Language features ---

    #[test]
    fn test_if_then_else_true() {
        assert_bool("if true then true else false");
    }

    #[test]
    fn test_if_then_else_false() {
        assert_bool("if false then false else true");
    }

    #[test]
    fn test_if_then_else_nested() {
        assert_bool("if true then (if false then false else true) else false");
    }

    #[test]
    fn test_object_field_access() {
        assert_bool("{a: 1}.a == 1");
        assert_bool(r#"{a: 1}["a"] == 1"#);
    }

    #[test]
    fn test_object_computed_field() {
        assert_bool(r#"local k = "x"; {[k]: 42}.x == 42"#);
    }

    #[test]
    fn test_object_plus_override() {
        assert_bool("{a: 1, b: 2} + {b: 99} == {a: 1, b: 99}");
    }

    #[test]
    fn test_object_field_override_syntax() {
        assert_bool("{a: 1} + {a+: 10} == {a: 11}");
    }

    #[test]
    fn test_object_hidden_field() {
        assert_bool("local o = {a:: 1, b: 2}; std.objectFields(o) == [\"b\"]");
        assert_bool("local o = {a:: 1, b: 2}; std.objectFieldsAll(o) == [\"a\",\"b\"]");
    }

    #[test]
    fn test_self_reference() {
        assert_bool("{a: 1, b: self.a + 1}.b == 2");
    }

    #[test]
    fn test_super_reference() {
        assert_bool("local base = {x: 1}; (base + {x: super.x + 1}).x == 2");
    }

    #[test]
    fn test_array_indexing() {
        assert_bool("[10, 20, 30][1] == 20");
        assert_bool("[10, 20, 30][-1] == 30");
    }

    #[test]
    fn test_array_slicing() {
        assert_bool("[1,2,3,4,5][1:3] == [2,3]");
        assert_bool("[1,2,3,4,5][:2] == [1,2]");
        assert_bool("[1,2,3,4,5][3:] == [4,5]");
        assert_bool("[1,2,3,4,5][::2] == [1,3,5]");
    }

    #[test]
    fn test_string_indexing_slicing() {
        assert_bool(r#""hello"[1] == "e""#);
        assert_bool(r#""hello"[1:3] == "el""#);
    }

    #[test]
    fn test_function_basic() {
        assert_bool("local f = function(x) x * 2; f(5) == 10");
    }

    #[test]
    fn test_function_default_param() {
        assert_bool("local f = function(x, y=10) x + y; f(5) == 15 && f(5, 20) == 25");
    }

    #[test]
    fn test_closure_capture() {
        assert_bool("local x = 5; local f = function() x; f() == 5");
    }

    #[test]
    fn test_closure_escaping() {
        assert_bool("local make = function(n) function() n; local f = make(42); f() == 42");
    }

    #[test]
    fn test_shared_upvalue() {
        assert_bool("local x = 1; local f = function() x; local g = function() x; f() == g()");
    }

    #[test]
    fn test_array_comprehension_basic() {
        assert_bool("[x * 2 for x in [1,2,3]] == [2,4,6]");
    }

    #[test]
    fn test_array_comprehension_filter() {
        assert_bool("[x for x in [1,2,3,4] if x > 2] == [3,4]");
    }

    #[test]
    fn test_object_comprehension_basic() {
        assert_bool(r#"{[k]: k + "!" for k in ["a","b"]} == {a: "a!", b: "b!"}"#);
    }

    #[test]
    fn test_object_comprehension_null_key() {
        // null key means the field is skipped
        assert_bool(r#"std.objectFields({[if false then "x" else null]: 1}) == []"#);
    }

    #[test]
    fn test_error_expression() {
        assert_err(r#"error "boom""#, "boom");
    }

    #[test]
    fn test_assert_pass() {
        assert_bool("assert true; true");
    }

    #[test]
    fn test_assert_fail() {
        assert_err("assert false : \"bad\"; true", "bad");
    }

    #[test]
    fn test_tailstrict_no_stack_overflow() {
        // Tail-recursive countdown — would overflow without tail call opt
        let src = "local f = function(n) if n == 0 then 0 else tailstrict f(n - 1); f(10000) == 0";
        assert_bool(src);
    }

    #[test]
    fn test_lazy_eval_unevaluated_branch() {
        // The error branch should never execute
        assert_bool("if true then true else error \"should not run\"");
    }

    #[test]
    fn test_local_shadowing_pipeline() {
        assert_bool("local x = 1; local x = 2; x == 2");
    }

    #[test]
    fn test_local_function_sugar_pipeline() {
        assert_bool("local f(x) = x + 1; f(5) == 6");
    }

    #[test]
    fn test_root_dollar_reference() {
        assert_bool("{x: 1, y: $.x + 1}.y == 2");
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:virtual_machine_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "test: add VM pipeline tests for language features (if, objects, arrays, functions, comprehensions)"
```

---

## Task 10: virtual_machine.rs — Error Paths and Serialization

**Files:**
- Modify: `src/virtual_machine.rs`

- [ ] **Step 1: Add VM error path and serialization tests**

```rust
    // --- VM error paths ---

    #[test]
    fn test_type_error_add_number_to_object() {
        assert_err("1 + {}", "cannot");
    }

    #[test]
    fn test_type_error_subtract_string() {
        assert_err(r#""a" - 1"#, "");
    }

    #[test]
    fn test_array_index_out_of_bounds() {
        assert_err("[1,2,3][10]", "");
    }

    #[test]
    fn test_object_field_not_found() {
        assert_err(r#"{a:1}.b"#, "");
    }

    #[test]
    fn test_call_non_function() {
        assert_err("1()", "");
    }

    #[test]
    fn test_function_too_many_args() {
        assert_err("local f = function(x) x; f(1, 2)", "");
    }

    #[test]
    fn test_function_too_few_args() {
        assert_err("local f = function(x, y) x + y; f(1)", "");
    }

    // --- value_to_json serialization ---

    #[test]
    fn test_value_to_json_null() {
        let chunk = create_test_chunk();
        let mm = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, mm);
        let mut visited = std::collections::HashSet::new();
        let json = vm.value_to_json(&Value::Null, &mut visited);
        assert_eq!(json, serde_json::Value::Null);
    }

    #[test]
    fn test_value_to_json_bool() {
        let chunk = create_test_chunk();
        let mm = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, mm);
        let mut visited = std::collections::HashSet::new();
        assert_eq!(vm.value_to_json(&Value::Boolean(true), &mut visited), serde_json::Value::Bool(true));
        let mut visited = std::collections::HashSet::new();
        assert_eq!(vm.value_to_json(&Value::Boolean(false), &mut visited), serde_json::Value::Bool(false));
    }

    #[test]
    fn test_value_to_json_number() {
        let chunk = create_test_chunk();
        let mm = MemoryManager::new();
        let mut vm = VirtualMachine::new(chunk, mm);
        let mut visited = std::collections::HashSet::new();
        let json = vm.value_to_json(&Value::Number(42.0), &mut visited);
        assert_eq!(json, serde_json::json!(42.0));
    }

    #[test]
    fn test_value_to_json_string() {
        let chunk = create_test_chunk();
        let mut mm = MemoryManager::new();
        let idx = mm.allocate_string("hello").index;
        let mut vm = VirtualMachine::new(chunk, mm);
        let mut visited = std::collections::HashSet::new();
        let json = vm.value_to_json(&Value::String(idx), &mut visited);
        assert_eq!(json, serde_json::json!("hello"));
    }

    #[test]
    fn test_value_to_json_array() {
        let chunk = create_test_chunk();
        let mut mm = MemoryManager::new();
        let arr = mm.allocate_array(vec![Value::Number(1.0), Value::Number(2.0)]).index;
        let mut vm = VirtualMachine::new(chunk, mm);
        let mut visited = std::collections::HashSet::new();
        let json = vm.value_to_json(&Value::Array(arr), &mut visited);
        assert_eq!(json, serde_json::json!([1.0, 2.0]));
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:virtual_machine_test
```

Expected: PASS.

- [ ] **Step 3: Check overall coverage**

```bash
bazel coverage //... --combined_report=lcov 2>/dev/null
python3 - <<'EOF'
with open('/home/scott/.cache/bazel/_bazel_scott/be1bb3f19226281555ee5a2094c7126b/execroot/_main/bazel-out/_coverage/_coverage_report.dat') as f:
    content = f.read()
targets = ['native.rs', 'virtual_machine.rs', 'compiler.rs', 'chunk.rs', 'scanner.rs']
cur = None
stats = {}
for line in content.split('\n'):
    if line.startswith('SF:'):
        cur = next((t for t in targets if line.endswith('src/' + t)), None)
        if cur: stats[cur] = [0, 0]
    elif cur and line.startswith('DA:'):
        p = line[3:].split(',')
        stats[cur][1] += 1
        if int(p[1]) > 0: stats[cur][0] += 1
    elif line == 'end_of_record':
        cur = None
for t in targets:
    if t in stats:
        h, total = stats[t]
        print(f"{t}: {h/total*100:.1f}% ({h}/{total})")
EOF
```

- [ ] **Step 4: Commit**

```bash
git add src/virtual_machine.rs
git commit -m "test: add VM error-path and value_to_json serialization tests"
```

---

## Task 11: compiler.rs — Uncovered Compile Paths

**Files:**
- Modify: `src/compiler_integration_test.rs`
- Modify: `src/compiler.rs` (error-path unit tests)

- [ ] **Step 1: Add integration tests to `compiler_integration_test.rs`**

Inside the existing `integration_tests` module, add:

```rust
    fn assert_compiles(source: &str) {
        let mut scanner = crate::scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).expect("compile failed");
        assert!(!chunk.is_empty());
    }

    fn compile_err(source: &str) -> String {
        let mut scanner = crate::scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        compiler.compile(&mut memory_manager).unwrap_err().message
    }

    #[test]
    fn test_object_literal_compilation() {
        assert_compiles("{a: 1, b: 2}");
    }

    #[test]
    fn test_object_hidden_field_compilation() {
        assert_compiles("{a:: 1}");
    }

    #[test]
    fn test_object_forced_field_compilation() {
        assert_compiles("{a::: 1}");
    }

    #[test]
    fn test_object_override_syntax_compilation() {
        assert_compiles("{a: 1} + {a+: 10}");
    }

    #[test]
    fn test_object_comprehension_compilation() {
        assert_compiles("{[k]: 1 for k in [\"a\",\"b\"]}");
    }

    #[test]
    fn test_assert_compilation() {
        assert_compiles("assert true; 1");
    }

    #[test]
    fn test_assert_with_message_compilation() {
        assert_compiles("assert 1 == 1 : \"fail\"; true");
    }

    #[test]
    fn test_function_with_defaults_compilation() {
        assert_compiles("local f(x, y=10) = x + y; f(5)");
    }

    #[test]
    fn test_tailstrict_compilation() {
        assert_compiles("local f(n) = if n == 0 then 0 else tailstrict f(n-1); f(5)");
    }

    #[test]
    fn test_self_compilation() {
        assert_compiles("{a: 1, b: self.a}");
    }

    #[test]
    fn test_super_compilation() {
        assert_compiles("local base = {x:1}; base + {x: super.x + 1}");
    }

    #[test]
    fn test_dollar_compilation() {
        assert_compiles("{a: 1, b: $.a}");
    }

    #[test]
    fn test_import_compilation() {
        // Import resolution happens at runtime; compilation itself should succeed
        assert_compiles("import \"nonexistent.jsonnet\"");
    }

    #[test]
    fn test_importstr_compilation() {
        assert_compiles("importstr \"nonexistent.txt\"");
    }

    #[test]
    fn test_importbin_compilation() {
        assert_compiles("importbin \"nonexistent.bin\"");
    }

    #[test]
    fn test_conditional_compilation() {
        assert_compiles("if true then 1 else 2");
    }

    #[test]
    fn test_error_expr_compilation() {
        assert_compiles("if false then error \"boom\" else 1");
    }

    #[test]
    fn test_array_comprehension_filter_compilation() {
        assert_compiles("[x for x in [1,2,3] if x > 1]");
    }
```

- [ ] **Step 2: Add error-path tests to `src/compiler.rs`'s `mod tests` block**

Find the existing `mod tests` block and add:

```rust
    fn compile_source_err(source: &str) -> String {
        let mut scanner = scanner::Scanner::new(source, "test.jsonnet");
        let mut mm = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        compiler.compile(&mut mm).unwrap_err().message
    }

    #[test]
    fn test_duplicate_param_error() {
        let msg = compile_source_err("function(x, x) x");
        assert!(msg.contains("duplicate") || msg.contains("already"), "got: {}", msg);
    }

    #[test]
    fn test_super_outside_object_error() {
        let msg = compile_source_err("super.x");
        assert!(!msg.is_empty());
    }
```

- [ ] **Step 3: Run tests**

```bash
bazel test //:compiler_test
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/compiler_integration_test.rs src/compiler.rs
git commit -m "test: add compiler integration tests for objects, comprehensions, assert, import, tailstrict"
```

---

## Task 12: chunk.rs — Disassembler and Large Operand Tests

**Files:**
- Modify: `src/chunk.rs`

- [ ] **Step 1: Add disassembler coverage tests to `mod tests`**

Inside the existing `mod tests` block, add:

```rust
    #[test]
    fn test_debug_compilation_produces_output() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode(Opcode::LoadNull, 0..5);
        chunk.write_opcode(Opcode::LoadTrue, 5..10);
        chunk.write_opcode(Opcode::LoadFalse, 10..15);
        chunk.write_opcode(Opcode::Return, 15..20);

        // debug_compilation returns a Report — just check it builds without panic
        let _report = chunk.debug_compilation();
    }

    #[test]
    fn test_write_opcode_u16_boundary() {
        let mut chunk = Chunk::new("test.jsonnet");
        // Test value at u8 boundary (256 requires u16)
        chunk.write_opcode_u16(Opcode::LoadConst, 256, 0..1);
        assert_eq!(chunk.count(), 3); // opcode byte + 2 bytes for u16
        let read = chunk.read_u16(1).unwrap();
        assert_eq!(read, 256);
    }

    #[test]
    fn test_write_opcode_u16_max() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u16(Opcode::LoadConst, 65535, 0..1);
        assert_eq!(chunk.read_u16(1).unwrap(), 65535);
    }

    #[test]
    fn test_write_opcode_u32_large() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u32(Opcode::LoadConst, 100_000, 0..1);
        assert_eq!(chunk.count(), 5); // opcode + 4 bytes
        assert_eq!(chunk.read_u32(1).unwrap(), 100_000);
    }

    #[test]
    fn test_add_constant_deduplication() {
        let mut chunk = Chunk::new("test.jsonnet");
        let idx1 = chunk.add_constant(Value::Number(42.0));
        let idx2 = chunk.add_constant(Value::Number(42.0));
        // Same constant — same index
        assert_eq!(idx1, idx2);
        assert_eq!(chunk.constants.len(), 1);
    }

    #[test]
    fn test_add_constant_different_values() {
        let mut chunk = Chunk::new("test.jsonnet");
        let idx1 = chunk.add_constant(Value::Number(1.0));
        let idx2 = chunk.add_constant(Value::Number(2.0));
        assert_ne!(idx1, idx2);
        assert_eq!(chunk.constants.len(), 2);
    }

    #[test]
    fn test_get_span_for_written_opcode() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode(Opcode::LoadNull, 10..20);
        let span = chunk.get_span(0).unwrap();
        assert_eq!(*span, 10..20);
    }

    #[test]
    fn test_patch_i32() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_i32(0, 0..4); // placeholder
        let pos = chunk.count() - 4;
        chunk.patch_i32(pos, 12345);
        assert_eq!(chunk.read_i32(pos).unwrap(), 12345);
    }

    #[test]
    fn test_write_opcode_u8_u8() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u8_u8(Opcode::LoadNull, 7, 9, 0..1);
        assert_eq!(chunk.count(), 3);
        assert_eq!(chunk.read_u8(1).unwrap(), 7);
        assert_eq!(chunk.read_u8(2).unwrap(), 9);
    }

    #[test]
    fn test_write_opcode_u16_u8() {
        let mut chunk = Chunk::new("test.jsonnet");
        chunk.write_opcode_u16_u8(Opcode::LoadNull, 1000, 5, 0..1);
        assert_eq!(chunk.count(), 4);
        assert_eq!(chunk.read_u16(1).unwrap(), 1000);
        assert_eq!(chunk.read_u8(3).unwrap(), 5);
    }

    #[test]
    fn test_read_beyond_end_returns_none() {
        let chunk = Chunk::new("test.jsonnet");
        assert!(chunk.read_u8(0).is_none());
        assert!(chunk.read_u16(0).is_none());
        assert!(chunk.read_u32(0).is_none());
        assert!(chunk.read_i32(0).is_none());
    }

    #[test]
    fn test_native_func_id_from_u16_roundtrip() {
        // Verify a few critical NativeFuncId roundtrips
        assert_eq!(NativeFuncId::from_u16(0), Some(NativeFuncId::Type));
        assert_eq!(NativeFuncId::from_u16(2), Some(NativeFuncId::Abs));
        assert_eq!(NativeFuncId::from_u16(9999), None);
    }

    #[test]
    fn test_value_display_all_types() {
        // Exercise Value Display/Debug formatting for coverage
        let _ = format!("{:?}", Value::Null);
        let _ = format!("{:?}", Value::Boolean(true));
        let _ = format!("{:?}", Value::Number(1.5));
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:chunk_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/chunk.rs
git commit -m "test: add chunk.rs disassembler, large-operand, and deduplication tests"
```

---

## Task 13: scanner.rs — Text Blocks, Verbatim Strings, Unicode, Error Paths

**Files:**
- Modify: `src/scanner.rs`

- [ ] **Step 1: Add tests to `mod tests`**

Inside the existing `mod tests` block, add:

```rust
    fn scan_all_ok(input: &str) -> Vec<Token> {
        let mut scanner = Scanner::new(input, "test");
        scanner.scan_all().unwrap().into_iter().map(|t| t.token).collect()
    }

    fn scan_all_err(input: &str) -> Vec<ScanError> {
        let mut scanner = Scanner::new(input, "test");
        scanner.scan_all().unwrap_err()
    }

    #[test]
    fn test_text_block_basic() {
        let input = "|||\n  hello\n  world\n|||";
        let tokens = scan_all_ok(input);
        assert!(matches!(&tokens[0], Token::String(s) if s == "hello\nworld\n"));
    }

    #[test]
    fn test_text_block_strip_newline() {
        let input = "|||-\n  hello\n|||";
        let tokens = scan_all_ok(input);
        assert!(matches!(&tokens[0], Token::String(s) if s == "hello"));
    }

    #[test]
    fn test_text_block_missing_newline_error() {
        let mut scanner = Scanner::new("|||   no-newline", "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_text_block_unterminated_error() {
        let mut scanner = Scanner::new("|||\n  hello\n", "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_text_block_mismatched_indent_error() {
        let input = "|||\n  good\n bad\n|||";
        let mut scanner = Scanner::new(input, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_verbatim_string_double_quote_escape() {
        let tokens = scan_all_ok(r#"@"foo""bar""#);
        assert!(matches!(&tokens[0], Token::String(s) if s == r#"foo"bar"#));
    }

    #[test]
    fn test_verbatim_string_single_quote_escape() {
        let tokens = scan_all_ok("@'it''s'");
        assert!(matches!(&tokens[0], Token::String(s) if s == "it's"));
    }

    #[test]
    fn test_unicode_escape_basic() {
        let tokens = scan_all_ok(r#""\u0041""#); // A
        assert!(matches!(&tokens[0], Token::String(s) if s == "A"));
    }

    #[test]
    fn test_unicode_surrogate_pair() {
        // U+1F600 GRINNING FACE = \uD83D\uDE00
        let tokens = scan_all_ok(r#""\uD83D\uDE00""#);
        assert!(matches!(&tokens[0], Token::String(s) if s == "\u{1F600}"));
    }

    #[test]
    fn test_unicode_unpaired_high_surrogate_error() {
        let mut scanner = Scanner::new(r#""\uD83D""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_unicode_invalid_low_surrogate_error() {
        let mut scanner = Scanner::new(r#""\uD83D\u0041""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
    }

    #[test]
    fn test_block_comment_unterminated() {
        let errors = scan_all_err("/* unterminated");
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("Unterminated"));
    }

    #[test]
    fn test_is_incomplete_input_true() {
        let err = ScanError::new(0..1, "Unexpected end of input".to_string(), "test".to_string());
        assert!(err.is_incomplete_input());

        let err2 = ScanError::new(0..1, "Unterminated string".to_string(), "test".to_string());
        assert!(err2.is_incomplete_input());
    }

    #[test]
    fn test_is_incomplete_input_false() {
        let err = ScanError::new(0..1, "Unexpected character 'x'".to_string(), "test".to_string());
        assert!(!err.is_incomplete_input());
    }

    #[test]
    fn test_into_report_with_cause() {
        let cause = ScanError::new(0..3, "root cause".to_string(), "test".to_string());
        let mut outer = ScanError::new(5..8, "outer error".to_string(), "test".to_string());
        outer.cause = Some(Box::new(cause));

        let (report, source_ids) = outer.into_report();
        let _ = report; // just verify it doesn't panic
        assert_eq!(source_ids.len(), 1);
    }

    #[test]
    fn test_save_and_restore_position() {
        let mut scanner = Scanner::new("hello world", "test");
        scanner.advance(); // consume 'h'
        let checkpoint = scanner.save_position();
        scanner.advance(); // consume 'e'
        scanner.advance(); // consume 'l'
        scanner.restore_position(checkpoint);
        // Should be back at 'e'
        let result = scanner.scan_next().unwrap();
        assert!(matches!(result.token, Token::Identifier(s) if s == "ello"));
    }

    #[test]
    fn test_collected_strings_after_scan() {
        let mut scanner = Scanner::new(r#""hello" "world""#, "test");
        scanner.scan_all().unwrap();
        assert_eq!(scanner.collected_strings().len(), 2);
    }

    #[test]
    fn test_invalid_escape_sequence() {
        let mut scanner = Scanner::new(r#""\q""#, "test");
        let result = scanner.scan_next();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Invalid escape"));
    }

    #[test]
    fn test_number_invalid_exponent() {
        let errors = scan_all_err("1e");
        assert!(!errors.is_empty());
    }
```

- [ ] **Step 2: Run tests**

```bash
bazel test //:scanner_test
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/scanner.rs
git commit -m "test: add scanner.rs text block, verbatim string, unicode, and error-path tests"
```

---

## Task 14: Final Coverage Check and Gap Filling

**Files:**
- Any files below 85%

- [ ] **Step 1: Measure coverage**

```bash
bazel coverage //... --combined_report=lcov 2>/dev/null
python3 - <<'EOF'
with open('/home/scott/.cache/bazel/_bazel_scott/be1bb3f19226281555ee5a2094c7126b/execroot/_main/bazel-out/_coverage/_coverage_report.dat') as f:
    content = f.read()
targets = ['native.rs', 'virtual_machine.rs', 'compiler.rs', 'chunk.rs', 'scanner.rs']
cur = None
stats = {}
for line in content.split('\n'):
    if line.startswith('SF:'):
        cur = next((t for t in targets if line.endswith('src/' + t)), None)
        if cur: stats[cur] = [0, 0]
    elif cur and line.startswith('DA:'):
        p = line[3:].split(',')
        stats[cur][1] += 1
        if int(p[1]) > 0: stats[cur][0] += 1
    elif line == 'end_of_record':
        cur = None
for t in targets:
    if t in stats:
        h, total = stats[t]
        pct = h/total*100
        status = "✓" if pct >= 85 else "✗ NEEDS WORK"
        print(f"{status} {t}: {pct:.1f}% ({h}/{total})")
EOF
```

- [ ] **Step 2: For each file below 85%, find remaining uncovered lines**

```bash
python3 - <<'EOF'
with open('/home/scott/.cache/bazel/_bazel_scott/be1bb3f19226281555ee5a2094c7126b/execroot/_main/bazel-out/_coverage/_coverage_report.dat') as f:
    content = f.read()
# Replace native.rs with whatever file needs attention
target = 'src/native.rs'
cur = False
uncov = []
for line in content.split('\n'):
    if line.startswith('SF:') and line.endswith(target):
        cur = True
    elif cur and line.startswith('DA:'):
        p = line[3:].split(',')
        if int(p[1]) == 0:
            uncov.append(int(p[0]))
    elif line == 'end_of_record':
        cur = False
print(f"Uncovered lines in {target}: {uncov[:50]}")
EOF
```

- [ ] **Step 3: Read uncovered lines and add targeted tests**

For each uncovered block, read the source to understand what's there and add a minimal test. Follow the same patterns established in Tasks 2–13.

- [ ] **Step 4: Run all tests**

```bash
bazel test //...
```

Expected: all tests pass.

- [ ] **Step 5: Run format and lint checks**

```bash
bazel build --config=rustfmt //...
bazel build --config=clippy //...
```

Expected: no errors or warnings.

- [ ] **Step 6: Final commit**

```bash
git add -p  # stage only test additions
git commit -m "test: final gap-fill tests to reach 85% coverage across all files"
```

---

## Success Criteria

- `bazel coverage //...` reports ≥85% for `native.rs`, `virtual_machine.rs`, `compiler.rs`, `chunk.rs`, `scanner.rs`
- `bazel test //...` passes
- `bazel build --config=clippy //...` passes
- `bazel build --config=rustfmt //...` passes
