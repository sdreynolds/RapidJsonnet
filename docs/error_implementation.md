# Error Expression Implementation

## Complete Implementation for Error Expression Support

**Status: ✅ COMPLETED**

This document describes the implementation of error expressions in RapidJsonnet, following the Jsonnet specification.

### Phase 1: Compiler Changes (`src/compiler.rs`)

**In `parse_prefix()` method:**
- Add `Token::Error` case that:
  1. Consumes the `error` token
  2. Calls `self.parse_expr(0, memory_manager)?` to parse the entire remaining expression (precedence 0 = consume everything to the right, following spec)
  3. Emits `Opcode::Error` with the error token's span
  4. Pushes `ExpressionType::Unknown` to type stack (since error never returns)

### Phase 2: Virtual Machine Changes (`src/virtual_machine.rs`)

**In the main execution loop match statement:**
- Add `Opcode::Error` case that:
  1. Pops the expression result from the stack
  2. Converts it to string using existing `value_to_json()` + `to_string()` (matches Jsonnet spec's tostring() behavior)
  3. Returns `RuntimeError` with the converted string as message and error keyword span

### Phase 3: Testing Implementation

**Unit Tests in `src/compiler.rs`:**
- Test that `error "message"` compiles to: `LoadConst(message_index)`, `Error`
- Test that `error (1 + 2)` compiles to: `LoadConst(1)`, `LoadConst(2)`, `Add`, `Error`

**Unit Tests in `src/virtual_machine.rs`:**
- Test VM with chunk containing `LoadConst("test")`, `Error` returns RuntimeError with message "test"
- Test VM with chunk containing `LoadConst(42)`, `Error` returns RuntimeError with message "42"
- Test VM with chunk containing `LoadTrue`, `Error` returns RuntimeError with message "true"

**End-to-End Tests:**
- `end2end/test_error_string.jsonnet`: `error "test message"`
- `end2end/test_error_number.jsonnet`: `error 42`
- `end2end/test_error_expression.jsonnet`: `error ("prefix " + "suffix")`

**Manual Testing with Bazel:**
```bash
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/test_error_string.jsonnet
# Should fail with: "test message"

bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/test_error_number.jsonnet
# Should fail with: "42"
```

## ✅ Implementation Results

### Compiler Implementation (`src/compiler.rs`)

**Added in `parse_prefix()` method (lines 255-262):**
```rust
Token::Error => {
    self.parser.advance()?; // consume 'error'
    // Parse the error expression with precedence 0 to consume everything to the right
    self.parse_expr(0, memory_manager)?;
    self.emit_opcode(Opcode::Error, token.span);
    // Error expressions never return a value
    self.push_type(ExpressionType::Unknown);
}
```

### Virtual Machine Implementation (`src/virtual_machine.rs`)

**Added in main execution loop (lines 629-644):**
```rust
Opcode::Error => {
    // Pop the error message value from the stack
    let error_value = self.pop()?;

    // Convert the value to string using existing JSON conversion
    let mut visited = std::collections::HashSet::new();
    let json_value = self.value_to_json(&error_value, &mut visited)?;
    let error_message = json_value.to_string();

    // Return RuntimeError with the converted message
    return Err(RuntimeError {
        span: self.get_current_span(),
        message: error_message,
        source_id: self.current_chunk().source_id.to_string(),
    });
}
```

### Test Implementation

**Unit Tests Added:**
- **Compiler tests**: `test_error_string_literal()`, `test_error_expression()` (lines 874-912 in `src/compiler.rs`)
- **VM tests**: `test_error_string_execution()`, `test_error_number_execution()`, `test_error_boolean_execution()` (lines 1203-1258 in `src/virtual_machine.rs`)

**End-to-End Test Files:**
- `end2end/test_error_string.jsonnet`: `error "test message"`
- `end2end/test_error_number.jsonnet`: `error 42`
- `end2end/test_error_expression.jsonnet`: `error ("prefix " + "suffix")`

### Verification Results

**Unit Tests:** ✅ All pass
```bash
bazel test //:compiler_test      # ✅ PASSED
bazel test //:virtual_machine_test # ✅ PASSED (27/27 tests)
```

**Manual Testing Results:**

1. **String Error:**
   ```bash
   bazel run //:main -- /path/to/test_error_string.jsonnet
   # ❌ Runtime error: "test message"
   ```

2. **Number Error:**
   ```bash
   bazel run //:main -- /path/to/test_error_number.jsonnet
   # ❌ Runtime error: 42.0  (correct JSON representation)
   ```

3. **Expression Error:**
   ```bash
   bazel run //:main -- /path/to/test_error_expression.jsonnet
   # ❌ Runtime error: "prefix suffix"  (expression evaluated first)
   ```

### Implementation Details

- **Precedence**: Parse with precedence 0 to consume everything to the right (per Jsonnet spec)
- **String conversion**: Use existing `value_to_json()` then `serde_json::Value::to_string()`
- **Error span**: Use the `error` keyword location for RuntimeError span
- **Type tracking**: Set `ExpressionType::Unknown` after parsing error expression
- **Error format**: Just the converted string, no prefix
- **Integration**: Use existing `RuntimeError` infrastructure

### Jsonnet Specification Compliance

The implementation follows the Jsonnet spec exactly:
- **Expression Evaluation**: The error argument is fully evaluated to a Jsonnet value before error handling
- **String Conversion**: Non-string values are converted using the same `tostring()` mechanism as string concatenation
- **Precedence**: The `error` keyword greedily consumes everything to its right, as specified for statement-like constructs
- **Error Semantics**: Error expressions cause "stuck execution" (runtime errors) as per the specification