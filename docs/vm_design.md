Virtual Machine
=====================

The virtual machine will be responsible for executing the compiled slice of u8 bytes. The machine as a method `interpret` takes as input the string slice from reading the main jsonnet file. The virutal machine calls the `Compiler` which produces an `ObjectFunction` which contains a slice of u8 bytes called a `chunk`. This function is pushed onto the virtual machine's stack and a root `CallFrame` is constructed. Finally the `interpret` method now calls the `run` method to execute the code. The `run` method returns an `Anyhow::Result` that presents either

1. INTERPRET_OK,
2. INTERPRET_RUNTIME_ERROR

The `interpret` method can return `INTERPRET_COMPILE_ERROR` when `Compiler` returns an `Error` result.

The `run` method is an infinite loop that exits when it sees OP_RETURN opcode with an INTERPRET_OK. The `run` method moves the `CallFrame`'s `instruction_pointer` through the `chunk` bytes based on the instructions of the opcodes.

Lexing
------
