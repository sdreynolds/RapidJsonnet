# Local Variables Implementation Plan

## Overview
Implement support for Jsonnet `local` variables using compile-time stack slot calculation. Since Jsonnet variables are immutable, we can determine stack positions at compile time and never need to recalculate them.

## Design Decisions

### Stack Model
- **Single stack**: No separate frame/register system
- **Compile-time slot assignment**: Each local gets an absolute stack position when declared
- **Immutability**: Variables never move once placed; no SETVAR needed, only LOADVAR
- **Stack slot numbering**: Absolute position from stack bottom (0-indexed)

### Scope Model
- Each `local` statement creates a new scope depth
- Comma-separated bindings (`local x = 1, y = 2;`) share the same scope depth
- Consecutive `local` statements create nested scopes
- Scope depth starts at 0 (global/module level)

## Implementation Phases

---

## Phase 1: Compiler Data Structures

### 1.1 Add Local Tracking Struct

Location: `src/compiler.rs` after line 20

```rust
/// Represents a local variable in the compiler's tracking
#[derive(Debug, Clone)]
struct Local {
    name: String,           // Variable name
    depth: u32,            // Scope nesting level
    stack_slot: usize,     // Absolute position from stack bottom
}
```

### 1.2 Extend Compiler Struct

Location: `src/compiler.rs` at line 37-42

Add these fields to the `Compiler` struct:
```rust
pub struct Compiler<'a> {
    // ... existing fields ...

    /// Tracks all local variables currently in scope
    locals: Vec<Local>,

    /// Current scope nesting depth (0 = module level, 1+ = local scopes)
    scope_depth: u32,

    /// Tracks the expected stack size at compile time
    stack_size: usize,
}
```

### 1.3 Update Constructor

Location: `src/compiler.rs` at line 44-55

Add initialization in `Compiler::new()`:
```rust
Self {
    compiling_chunk,
    parser,
    type_stack: Vec::new(),
    constant_pool: HashMap::new(),
    locals: Vec::new(),          // NEW
    scope_depth: 0,              // NEW
    stack_size: 0,               // NEW
}
```

---

## Phase 2: Scope Management Methods

### 2.1 Begin Scope

Location: `src/compiler.rs` (new method in impl block)

```rust
/// Enter a new lexical scope (increments depth)
fn begin_scope(&mut self) {
    self.scope_depth += 1;
}
```

### 2.2 End Scope

Location: `src/compiler.rs` (new method in impl block)

```rust
/// Exit current scope, emitting Pop instructions for locals at this depth
fn end_scope(&mut self) -> Result<(), CompilerError> {
    // Pop all locals at current depth (in reverse declaration order)
    while let Some(local) = self.locals.last() {
        if local.depth == self.scope_depth {
            let span = self.current_span();
            self.emit_opcode(Opcode::Pop, span);
            self.stack_size -= 1;  // Track stack shrinkage
            self.locals.pop();
        } else {
            break;  // Reached locals from outer scope
        }
    }

    self.scope_depth -= 1;
    Ok(())
}
```

### 2.3 Declare Local Variable

Location: `src/compiler.rs` (new method in impl block)

```rust
/// Declare a new local variable at the current scope depth
/// The value must already be on the stack
fn declare_local(&mut self, name: String) -> Result<(), CompilerError> {
    // Check for duplicate in current scope
    for local in self.locals.iter().rev() {
        if local.depth < self.scope_depth {
            break;  // Reached outer scope
        }
        if local.name == name {
            return Err(self.make_error(
                self.current_span(),
                format!("Variable '{}' already declared in this scope", name),
            ));
        }
    }

    // Value is already on stack at position stack_size - 1
    let stack_slot = self.stack_size - 1;

    self.locals.push(Local {
        name,
        depth: self.scope_depth,
        stack_slot,
    });

    Ok(())
}
```

### 2.4 Resolve Local Variable

Location: `src/compiler.rs` (new method in impl block)

```rust
/// Resolve a variable name to its stack slot
/// Returns None if not found in local scope
fn resolve_local(&self, name: &str) -> Option<usize> {
    // Search from innermost to outermost scope (reverse order)
    for local in self.locals.iter().rev() {
        if local.name == name {
            return Some(local.stack_slot);
        }
    }
    None
}
```

---

## Phase 3: Parser Integration - Local Statements

### 3.1 Add Local Token Handling

Location: `src/compiler.rs` in `parse_prefix()` at line 282

Add case for `Token::Local`:

```rust
Token::Local => {
    self.parse_local_statement(memory_manager)?;
    // Local statement result is the body expression value
    // Type depends on body expression
    self.push_type(ExpressionType::Unknown);
}
```

### 3.2 Parse Local Statement Method

Location: `src/compiler.rs` (new method in impl block)

```rust
/// Parse: local x = expr, y = expr; body_expr
fn parse_local_statement(
    &mut self,
    memory_manager: &mut MemoryManager,
) -> Result<(), CompilerError> {
    self.parser.advance()?; // consume 'local'

    // Enter new scope for these locals
    self.begin_scope();

    // Parse comma-separated bindings
    loop {
        // Expect identifier
        let name_token = self.parser.current_token().cloned()
            .ok_or_else(|| self.unexpected_eof_error(self.current_span()))?;

        let var_name = match &name_token.token {
            Token::Identifier(name) => name.clone(),
            _ => {
                return Err(self.make_error(
                    name_token.span,
                    "Expected variable name after 'local'".to_string(),
                ));
            }
        };

        self.parser.advance()?; // consume identifier

        // Expect '='
        self.parser.consume(
            Token::Operator("=".to_string()),
            "Expected '=' after variable name",
        )?;

        // Parse binding expression
        self.parse_expr(0, memory_manager)?;
        self.stack_size += 1;  // Expression leaves value on stack

        // Declare the local (value is now on stack)
        self.declare_local(var_name)?;

        // Check for comma (more bindings) or semicolon (end of bindings)
        if let Some(token) = self.parser.current_token() {
            match &token.token {
                Token::Comma => {
                    self.parser.advance()?; // consume ','
                    continue; // Parse next binding
                }
                Token::Semicolon => {
                    self.parser.advance()?; // consume ';'
                    break; // Done with bindings
                }
                _ => {
                    return Err(self.make_error(
                        token.span.clone(),
                        "Expected ',' or ';' in local statement".to_string(),
                    ));
                }
            }
        } else {
            return Err(self.unexpected_eof_error(self.current_span()));
        }
    }

    // Parse body expression (with locals in scope)
    self.parse_expr(0, memory_manager)?;
    // Body expression result stays on stack

    // Exit scope - emit Pop for each local
    self.end_scope()?;

    Ok(())
}
```

---

## Phase 4: Parser Integration - Variable References

### 4.1 Add Identifier Token Handling

Location: `src/compiler.rs` in `parse_prefix()` at line 282

Add case for `Token::Identifier`:

```rust
Token::Identifier(name) => {
    let name_clone = name.clone();
    self.parser.advance()?; // consume identifier

    // Try to resolve as local variable
    if let Some(stack_slot) = self.resolve_local(&name_clone) {
        // Emit LoadVar with absolute stack slot
        let span = token.span;
        self.compiling_chunk.write_opcode_u16(
            Opcode::LoadVar,
            stack_slot as u16,
            span,
        );
        self.stack_size += 1;  // LoadVar pushes value
        self.push_type(ExpressionType::Unknown);
    } else {
        // Variable not found
        return Err(self.make_error(
            token.span,
            format!("Undefined variable '{}'", name_clone),
        ));
    }
}
```

---

## Phase 5: Virtual Machine Implementation

### 5.1 Implement LoadVar Opcode

Location: `src/virtual_machine.rs` in `interpret()` loop at line 144

Add case in the match statement:

```rust
Opcode::LoadVar => {
    let stack_slot = self.read_u16_operand()? as usize;

    // Validate stack slot
    if stack_slot >= self.stack.len() {
        return Err(RuntimeError {
            span: self.get_current_span(),
            message: format!(
                "Invalid stack slot {} (stack size: {})",
                stack_slot,
                self.stack.len()
            ),
            source_id: self.current_chunk().source_id.to_string(),
        });
    }

    // Copy value from slot to top of stack
    let value = self.stack[stack_slot].clone();
    self.push(value)?;
}
```

---

## Phase 6: Testing

### 6.1 Unit Tests in compiler.rs

Location: `src/compiler.rs` in `#[cfg(test)]` module

Add tests:

```rust
#[test]
fn test_simple_local() {
    // local x = 5; x
    let mut scanner = Scanner::new("local x = 5; x", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    // Should have constant for 5
    assert_eq!(chunk.constants.len(), 1);
    assert_eq!(chunk.constants[0], Value::Number(5.0));
}

#[test]
fn test_multiple_locals() {
    // local x = 1, y = 2; x + y
    let mut scanner = Scanner::new("local x = 1, y = 2; x + y", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    // Should have constants for 1 and 2
    assert_eq!(chunk.constants.len(), 2);
}

#[test]
fn test_local_using_local() {
    // local x = 1, y = x + 1; y
    let mut scanner = Scanner::new("local x = 1, y = x + 1; y", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    assert!(chunk.code.len() > 0);
}

#[test]
fn test_forward_reference_error() {
    // local x = y + 1, y = 5; x (should fail)
    let mut scanner = Scanner::new("local x = y + 1, y = 5; x", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let result = compiler.compile(&mut memory_manager);

    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("Undefined variable 'y'"));
}

#[test]
fn test_duplicate_local_error() {
    // local x = 1, x = 2; x (should fail)
    let mut scanner = Scanner::new("local x = 1, x = 2; x", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let result = compiler.compile(&mut memory_manager);

    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("already declared"));
}

#[test]
fn test_nested_local_scopes() {
    // local x = 1; local y = x + 1; y
    let mut scanner = Scanner::new("local x = 1; local y = x + 1; y", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    assert!(chunk.code.len() > 0);
}

#[test]
fn test_local_shadowing() {
    // local x = 1; local x = 2; x (shadowing in nested scope)
    let mut scanner = Scanner::new("local x = 1; local x = 2; x", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    assert!(chunk.code.len() > 0);
}

#[test]
fn test_local_with_object() {
    // local x = {awesome: true}; x
    let mut scanner = Scanner::new("local x = {awesome: true}; x", "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    assert!(chunk.code.len() > 0);
}

#[test]
fn test_local_with_nested_object() {
    let input = r#"local x = {
        awesome: true,
        nestedObj: {
            anotherNest: 45,
            someString: "this is great"
        }
    }; x"#;
    let mut scanner = Scanner::new(input, "test");
    let compiler = Compiler::new(&mut scanner, "test");
    let mut memory_manager = MemoryManager::new();
    let chunk = compiler.compile(&mut memory_manager).unwrap();

    assert!(chunk.code.len() > 0);
}
```

### 6.2 Unit Tests in virtual_machine.rs

Location: `src/virtual_machine.rs` in `#[cfg(test)]` module

```rust
#[test]
fn test_loadvar_opcode() {
    let mut chunk = create_test_chunk();

    // Push two values on stack
    let idx_1 = chunk.add_constant(Value::Number(10.0));
    let idx_2 = chunk.add_constant(Value::Number(20.0));

    chunk.write_opcode_u16(Opcode::LoadConst, idx_1 as u16, 0..5);  // stack[0] = 10
    chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 5..10); // stack[1] = 20
    chunk.write_opcode_u16(Opcode::LoadVar, 0, 10..15);              // Load from slot 0
    chunk.write_opcode(Opcode::Return, 15..20);

    let memory_manager = MemoryManager::new();
    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret().unwrap();

    // Should return value from slot 0 (10.0)
    assert_eq!(result, Value::Number(10.0));
}

#[test]
fn test_loadvar_multiple_slots() {
    let mut chunk = create_test_chunk();

    let idx_1 = chunk.add_constant(Value::Number(10.0));
    let idx_2 = chunk.add_constant(Value::Number(20.0));

    chunk.write_opcode_u16(Opcode::LoadConst, idx_1 as u16, 0..5);   // stack[0] = 10
    chunk.write_opcode_u16(Opcode::LoadConst, idx_2 as u16, 5..10);  // stack[1] = 20
    chunk.write_opcode_u16(Opcode::LoadVar, 0, 10..15);               // Load slot 0 (10)
    chunk.write_opcode_u16(Opcode::LoadVar, 1, 15..20);               // Load slot 1 (20)
    chunk.write_opcode(Opcode::Add, 20..25);                          // 10 + 20
    chunk.write_opcode(Opcode::Return, 25..30);

    let memory_manager = MemoryManager::new();
    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret().unwrap();

    assert_eq!(result, Value::Number(30.0));
}

#[test]
fn test_loadvar_invalid_slot() {
    let mut chunk = create_test_chunk();

    chunk.write_opcode_u16(Opcode::LoadVar, 99, 0..5);  // Invalid slot
    chunk.write_opcode(Opcode::Return, 5..10);

    let memory_manager = MemoryManager::new();
    let mut vm = VirtualMachine::new(chunk, memory_manager);
    let result = vm.interpret();

    assert!(result.is_err());
    assert!(result.unwrap_err().message.contains("Invalid stack slot"));
}
```

### 6.3 End-to-End Tests

Location: `end2end/` directory (create new files)

**File: `end2end/local_simple.jsonnet`**
```jsonnet
local x = 5;
x
```

**File: `end2end/local_multiple.jsonnet`**
```jsonnet
local x = 1, y = 2;
x + y
```

**File: `end2end/local_nested.jsonnet`**
```jsonnet
local x = 1;
local y = x + 1;
y
```

**File: `end2end/local_object.jsonnet`**
```jsonnet
local x = {awesome: true, nestedObj: {anotherNest: 45, someString: "this is great"}};
x
```

**File: `end2end/local_object_access.jsonnet`**
```jsonnet
local obj = {name: "test", value: 42};
obj.value
```

**File: `end2end/local_shadowing.jsonnet`**
```jsonnet
local x = 10;
local x = 20;
x
```

**Run tests with:**
```bash
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_simple.jsonnet
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_multiple.jsonnet
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_nested.jsonnet
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_object.jsonnet
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_object_access.jsonnet
bazel run //:main -- /home/scott/Projects/RapidJsonnet/end2end/local_shadowing.jsonnet
```

---

## Phase 7: Edge Cases & Error Handling

### 7.1 Error Cases to Handle

1. **Undefined variable**: `x + 1` when `x` not declared
2. **Forward reference**: `local x = y, y = 1; x`
3. **Duplicate in same scope**: `local x = 1, x = 2; x`
4. **Missing semicolon**: `local x = 1 x`
5. **Missing equals**: `local x 1; x`
6. **Empty local**: `local ; 5`

### 7.2 Stack Integrity

- Ensure `stack_size` tracking matches actual VM stack
- Verify Pop instructions are emitted correctly
- Check that body expression result stays on stack after popping locals

---

## Implementation Order

1. ✅ Phase 1: Data structures
2. ✅ Phase 2: Scope management methods
3. ✅ Phase 3: Parse local statements
4. ✅ Phase 4: Parse variable references
5. ✅ Phase 5: VM LoadVar implementation
6. ✅ Phase 6.1: Compiler unit tests
7. ✅ Phase 6.2: VM unit tests
8. ✅ Phase 6.3: End-to-end tests
9. ✅ Phase 7: Edge cases & polish

---

## Success Criteria

- [x] All unit tests pass in compiler.rs
- [x] All unit tests pass in virtual_machine.rs
- [x] All end-to-end test files execute correctly
- [x] Error messages are clear and helpful
- [x] No memory leaks or stack corruption
- [x] Objects as local values work correctly
- [x] Nested objects as local values work correctly

---

## Implementation Results

### Key Implementation Changes

The implementation largely followed the plan with one critical adjustment:

#### Stack Slot Calculation Strategy (Modified)

**Original Plan**: Use `stack_size` tracking incremented for each expression
**Actual Implementation**: Use `locals.len()` for slot calculation

```rust
// Final implementation in declare_local()
let stack_slot = self.locals.len();  // Not self.stack_size - 1
```

**Reason**: Tracking `stack_size` across all expressions proved error-prone because binary operations like `Add` change stack depth (pop 2, push 1) but weren't being tracked. Using `locals.len()` is simpler and correct: each local occupies exactly one stack position, so the Nth local is at position N-1.

#### Stack Cleanup Strategy (Critical Fix)

**Challenge**: After evaluating the body expression, the stack contains `[local0, local1, ..., result]`. Simple `Pop` instructions remove from the top, which would remove the result instead of the locals.

**Solution**: Use Swap+Pop pattern in `end_scope()`:

```rust
fn end_scope(&mut self) {
    let span = self.current_span();

    while let Some(local) = self.locals.last() {
        if local.depth == self.scope_depth {
            // Swap result with local, then pop the local
            self.emit_opcode(Opcode::Swap, span.clone());
            self.emit_opcode(Opcode::Pop, span.clone());
            self.locals.pop();
        } else {
            break;
        }
    }

    self.scope_depth -= 1;
}
```

This bubbles the result down through the locals while removing each one, keeping the result on top.

### Test Results

All tests passing:

**Unit Tests:**
- ✅ test_simple_local - `local x = 5; x` → 5.0
- ✅ test_multiple_locals - `local x = 1, y = 2; x + y` → 3.0
- ✅ test_local_using_local - `local x = 1, y = x + 1; y` compiles
- ✅ test_forward_reference_error - Properly rejects undefined variables
- ✅ test_duplicate_local_error - Catches duplicate declarations
- ✅ test_nested_local_scopes - Nested scopes work correctly
- ✅ test_local_shadowing - Inner scope shadows outer scope
- ✅ test_local_with_object - Objects as locals work
- ✅ test_local_with_nested_object - Nested objects as locals work
- ✅ test_loadvar_opcode - VM correctly loads from stack slots
- ✅ test_loadvar_multiple_slots - Multiple LoadVar operations work
- ✅ test_loadvar_invalid_slot - Proper error for invalid slots

**End-to-End Tests:**
- ✅ local_simple.jsonnet → 5.0
- ✅ local_multiple.jsonnet → 3.0
- ✅ local_nested.jsonnet → 2.0
- ✅ local_object.jsonnet → `{"awesome":true,"nestedObj":{...}}`
- ✅ local_object_access.jsonnet → 42.0
- ✅ local_shadowing.jsonnet → 20.0

### Final Architecture

**Compiler Fields (Removed stack_size):**
```rust
pub struct Compiler<'a> {
    compiling_chunk: Chunk<'a>,
    parser: Parser<'a>,
    type_stack: Vec<ExpressionType>,
    constant_pool: HashMap<Value, u16>,
    locals: Vec<Local>,   // Tracks locals with absolute positions
    scope_depth: u32,     // Nesting level
}
```

**Local Structure:**
```rust
struct Local {
    name: String,       // Variable name
    depth: u32,         // Scope nesting level
    stack_slot: usize,  // Absolute position from stack bottom
}
```

**Bytecode Pattern for `local x = 1, y = 2; x + y`:**
```
LoadConst 1        // Push 1 (x's value)
LoadConst 2        // Push 2 (y's value)
LoadVar 0          // Copy x from slot 0
LoadVar 1          // Copy y from slot 1
Add                // 1 + 2 = 3
Swap               // Swap result with y
Pop                // Remove y
Swap               // Swap result with x
Pop                // Remove x
Return             // Return 3
```

---

## Notes

- **Immutability**: No SETVAR needed, simplifies implementation
- **Single pass**: Stack slots calculated once at declaration using `locals.len()`
- **No explicit stack_size tracking**: Removed from final implementation
- **Scope nesting**: Each `local` statement creates new depth level
- **LoadVar copies**: Values stay in their slots, LoadVar pushes a copy to top
- **Swap+Pop cleanup**: Critical pattern for preserving body result while removing locals
