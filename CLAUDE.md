# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

RapidJsonnet is a bytecode interpreter implementation of Jsonnet built with Rust and Bazel. The project implements a complete language pipeline: scanner → parser → compiler → virtual machine.

## Architecture

**Core Components:**
- `scanner.rs` - Lexical analysis, tokenizes Jsonnet source code
- `parser.rs` - Recursive descent parser that builds token streams
- `compiler.rs` - Pratt parser that compiles tokens to bytecode using precedence climbing
- `chunk.rs` - Bytecode container with constants pool and span tracking
- `virtual_machine.rs` - Stack-based VM that executes bytecode chunks
- `string_pool.rs` - String interning system for memory efficiency
- `main.rs` - CLI entry point supporting both file execution and REPL mode

**Key Design Patterns:**
- String interning for memory efficiency and fast equality comparisons
- SlotMap-based object storage with garbage collection
- Constant pooling to avoid duplicate values in bytecode
- Span tracking for precise error reporting with ariadne
- Expression type tracking for compile-time optimizations

## Development Commands

**Build and Test:**
```bash
bazel build //...          # Build all targets
bazel test //...           # Run all tests
bazel run //:main          # Run REPL mode
bazel run //:main file.jsonnet  # Execute jsonnet file
```

**Individual Components:**
```bash
bazel test //:scanner_test      # Test scanner
bazel test //:parser_test       # Test parser  
bazel test //:compiler_test     # Test compiler
bazel test //:virtual_machine_test  # Test VM
bazel test //:chunk_test        # Test chunk
bazel test //:string_pool_test  # Test string pool
```

**Code Quality:**
```bash
bazel build --config=rustfmt //...  # Format check
bazel build --config=clippy //...   # Lint check
```

**Debugging:**
The main binary outputs compilation visualization and execution traces. Use `stress_gc` feature for garbage collection stress testing.

## Operational Notes

- Uses Bazel with rules_rust for build system
- All targets use `stress_gc` crate feature for GC testing
- End-to-end tests in `end2end/` directory validate complete pipeline
- Documentation in `docs/` explains implementation details for each component
- Virtual machine has 65536 max stack size and configurable GC thresholds
- Error reporting uses ariadne for precise source location display