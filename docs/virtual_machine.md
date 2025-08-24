# Jsonnet Virtual machine
Create a virtual_machine.rs file and add it to the BUILD.bazel file. The VirtualMachine struct will have

- growable vector of chunks
- a `current_chunk` index
- a `program_counter` to know where it is in the the `current_chunk`
- a `stack` of `Value` objects

The constructor will take in the starting `chunk`, insert it into the `vector` of chunks and initilaze the `current_chunk` to that index and the `program_counter` to the first element in the `chunk`'s `code` property.

The `interpret` method will return the resulting `serde_json::Result` containing either the `serde_json::Value` or `serde_json::Error`. The method walks through the current chunk and only handles:

- LoadNull => no operands
- LoadTrue => no operands
- LoadFalse => no operands
- Add => no operands
- Sub => no operands
- Mul => no operands
- Div => no operands
- Lt => no operands
- Le => no operands
- Gt => no operands
- Ge => no operands
- Shl => no operands
- Shr => no operands
- BitAnd => no operands
- BitXor => no operands
- BitOr => no operands
- LogicalAnd => no operands
- LogicalOr => no operands
- Neg => no operands
- Pos => no operands
- Not => no operands
- BitNot => no operands
- Pop => no operands
- Dup => no operands
- Swap => no operands
- LoadConst => u16 operand will require combining two u8 into a single u16 value. This will be a index into the `constants` vector in `Chunk`. Move the program counter by three when handling `LoadConst` but only move the program counter by 1 for `opcode` with no operands.

It will manipulate the stack to return is either `serde_json::Value` or `RuntimeError`. The rest of the `opcode`s will result in a `RuntimeError`. When processing an `opcode`, combine `u8` and move the `program_counte` forward. Each `opcode` can have different size and number of `operand`s. Test to ensure the `operand` and the makes sense.

When processing `opcode`, when an `opcode` is found that is not recognized, return a `RuntimeError`. It is expected that the last `u8` in a `Chunk` is a `Return` `opcode`. `Return` `opcode` should return the `Value` at the top of the stack. Pop the value off the stack and return it and halt execution.

##  Stack State Validation
- Binary ops (Add, Sub, etc.): need exactly 2 values on stack
- Unary ops (Neg, Not, etc.): need exactly 1 value on stack

When the size of the stack is too small for the `opcode`, return a Stack Underflow `RuntimeError`

## RuntimeError Struct
The `RuntimeError` Struct will be a type alias for `ScanError` and therefore will have a `into_report`.

- Source span tracking for runtime errors (using chunk's span info)

## execute method
the `execute` function will be the main entrypoint. To start, this will take in a single `Chunk`, create the `VirtualMachine`, run `interpret` and in the event of a `RuntimeError` turn it into a report and print it to the screen. Otherwise print the returned `serde_json::Value`. When a `RuntimeError` occurs generate a stack trace that can be added to an `ariadne` and print out that report. In the event of a `RuntimeError` return an `Error` struct that signals that it failed and print a nice stack trace using `ariadne` library.

For this pass, stop execution whenever a `RuntimeError` occurs and do not attempt a recovery.

## Tests
construct a number of tests to ensure each of the `opcode` works and has coverage

## Value type system
For the value system, we will start with updating the type defintion from only `f64` to:
- `f64`
- `Boolean`
- `Null`

## Truthy values
Falsy Values:

    false: The boolean literal false is the only explicitly falsy value.
    null: The null value is considered falsy.
    Zero-valued numbers: The number 0 is considered falsy.

Truthy Values:

    true: The boolean literal true is the only explicitly truthy value.
    Non-zero numbers: Any number other than 0 (e.g., 1, -1, 0.5) is considered truthy.

## Stack considerations
Implementation Notes:

  - Use Vec<Value> with Vec::with_capacity(1024) for initial allocation
  - Check stack depth before each push operation
  - Provide clear "stack overflow" error messages with source spans
  - Consider making limits configurable for special use cases
  - Missing maximum stack size 65536
  - double stack size when capacity is reached
  - when operands are missing, raise a `RuntimeError` that represents the stack underflow
