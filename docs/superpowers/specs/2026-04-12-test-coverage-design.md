# Test Coverage Improvement Design

**Date:** 2026-04-12
**Goal:** Achieve 85% line coverage across all source files.

## Current State

| File | Coverage | Uncovered Lines | Notes |
|------|----------|----------------|-------|
| `native.rs` | 0.2% | ~3,791 | No unit tests, no `native_test` BUILD target |
| `virtual_machine.rs` | 15.4% | ~8,312 | 40 tests cover only basic arithmetic/stack ops |
| `compiler.rs` | 46.0% | ~1,822 | 38 tests miss objects, comprehensions, imports |
| `chunk.rs` | 47.8% | ~971 | Disassembler and serialization paths untested |
| `scanner.rs` | 64.6% | ~236 | Text blocks, verbatim strings, error paths missing |

## Key Constraint: What Counts for Coverage

Coverage is measured by `bazel coverage //...` which instruments `rust_test` targets only. The existing `sh_test` end2end `.jsonnet` files run as external processes and generate no LLVM coverage data for the Rust binaries. Coverage can only be improved by adding tests to `#[cfg(test)]` modules within `rust_test`-linked crates.

The established pattern for "integration-style" Rust tests is: compile a Jsonnet source string through the full `Scanner → Compiler → VirtualMachine::interpret()` pipeline within a `#[cfg(test)]` block. This exercises native functions, VM opcodes, and compiler paths — all counted toward coverage.

## Approach: File-by-File, Worst-First

Files are tackled in priority order by coverage gap. Each file reaches 85% before moving to the next. For each file: pipeline-level tests for happy paths, inline unit tests for error paths.

---

## File 1: native.rs (0.2% → 85%)

### 1a. BUILD.bazel Change

Add `native_integration_test.rs` to the `:native` library's `srcs` list, mirroring how `:compiler` includes `compiler_integration_test.rs`. Add a `native_test` rust_test target:

```python
rust_test(
    name = "native_test",
    crate = ":native",
)
```

### 1b. `src/native_integration_test.rs` — Pipeline Tests

One test per std function. Each test compiles a Jsonnet assertion snippet and verifies the VM returns `Value::Boolean(true)`. Uses the `Scanner → Compiler → VirtualMachine` pipeline.

> **Note:** The list below is aspirational. During implementation, verify each function name against the actual `NativeFuncId` enum in `chunk.rs` and the dispatch table in `call_native` in `native.rs` — skip any not yet implemented.

Functions to cover (happy paths):
- Math: `std.abs`, `std.floor`, `std.ceil`, `std.round`, `std.min`, `std.max`, `std.sign`, `std.clamp`, `std.hypot`, `std.deg2Rad`, `std.rad2Deg`, `std.pow`, `std.exp`, `std.log`, `std.sqrt`, `std.sin`, `std.cos`, `std.tan`, `std.asin`, `std.acos`, `std.atan`
- Type: `std.type`, `std.isArray`, `std.isBoolean`, `std.isNumber`, `std.isObject`, `std.isString`, `std.isNull`, `std.isFunction`
- String: `std.length` (strings), `std.substr`, `std.split`, `std.join`, `std.lines`, `std.stringChars`, `std.codepoint`, `std.char`, `std.toString`, `std.asciiUpper`, `std.asciiLower`, `std.startsWith`, `std.endsWith`, `std.findSubstr`, `std.strReplace`, `std.isEmpty`, `std.equalsIgnoreCase`, `std.escapeStringBash`, `std.escapeStringDollars`, `std.escapeStringJson`, `std.escapeStringPython`, `std.escapeStringXml`
- Parsing: `std.parseInt`, `std.parseOctal`, `std.parseHex`
- Encoding: `std.base64`, `std.base64Decode`, `std.base64DecodeBytes`
- Format: `std.format` (all format specifiers: `%s`, `%d`, `%f`, `%e`, `%g`, `%o`, `%x`, `%X`, `%c`, `%%`, named fields)
- Array: `std.length` (arrays), `std.range`, `std.reverse`, `std.sort`, `std.uniq`, `std.flatten`, `std.flatMap`, `std.filter`, `std.map`, `std.mapWithIndex`, `std.foldl`, `std.foldr`, `std.find`, `std.member`, `std.count`, `std.contains`, `std.all`, `std.any`, `std.sum`, `std.avg`, `std.remove`, `std.removeAt`, `std.makeArray`
- Object: `std.objectFields`, `std.objectFieldsAll`, `std.objectValues`, `std.objectValuesAll`, `std.objectHas`, `std.objectHasAll`, `std.get`, `std.mapObject`, `std.filterObject`, `std.groupBy`
- Set: `std.set`, `std.setUnion`, `std.setInter`, `std.setDiff`, `std.setMember`
- Manifest/Parse: `std.manifestJson`, `std.manifestJsonEx`, `std.manifestYamlDoc`, `std.manifestIni`, `std.manifestPython`, `std.manifestPythonVars`, `std.manifestTomlEx`, `std.parseJson`, `std.parseYaml`
- Other: `std.assertEqual`, `std.deepJoin`

### 1c. Inline Unit Tests in `native.rs`

Error-path tests calling `call_native` directly with a `MemoryManager`:
- Wrong argument types (e.g., `std.abs` with a string)
- Arity mismatches (more or fewer args than expected)
- `std.char` with out-of-range codepoint (> 0x10FFFF)
- `std.char` with surrogate codepoint (0xD800–0xDFFF)
- `std.codepoint` on multi-character string
- `std.substr` with out-of-bounds index
- `std.split` with empty separator
- `std.parseHex`/`std.parseOctal`/`std.parseInt` with invalid input
- `std.base64Decode` with invalid base64
- `std.assertEqual` mismatch (should produce RuntimeError)
- `coerce_to_sorted_array` with non-array, non-string value

---

## File 2: virtual_machine.rs (15.4% → 85%)

All tests added to the existing `mod tests` block.

### 2a. Pipeline Tests for Uncovered Language Features

Compile Jsonnet source strings and assert on the returned value or error:

- **Conditionals:** `if/then/else` with true condition, false condition, nested
- **Object construction:** literal fields, computed field names, `+:` override, `::` hidden fields, `:::` forced visibility
- **Field access:** `.` operator, `["field"]` index access, missing field error
- **Array indexing:** positive index, negative index, out-of-bounds error
- **Array slicing:** `arr[1:3]`, `arr[::2]`, open-ended slices
- **String indexing and slicing**
- **Local scoping:** shadowing, forward references in local blocks
- **Functions:** definition, call, default parameters, too-few/too-many args error
- **Closures:** upvalue capture, escaping closures, shared upvalues
- **`self` reference:** `self.field` inside object
- **`super` reference:** `base + { override }` with super access
- **`$` root reference:** top-level self reference
- **Object comprehension:** basic, with filter condition, null key (skips field)
- **Array comprehension:** basic, with filter, nested
- **`error` expression:** produces RuntimeError with correct message
- **`assert` statement:** passing assert, failing assert with custom message
- **`import`:** basic file import (using temp files), import caching
- **`importstr`:** imports file as string
- **`importbin`:** imports file as binary array
- **Tail calls:** `tailstrict` recursive function doesn't stack overflow
- **String `%` formatting:** `"Hello %s" % "world"`
- **Lazy evaluation:** unevaluated branches of `if` don't cause errors
- **`std.extVar`:** already tested, but add type-coerced variants

### 2b. Error-Path Unit Tests

Direct `VirtualMachine` construction (raw chunk building) for:
- Stack overflow (exceed 65536 stack depth)
- Type error in binary ops: number + object, string - number, etc.
- Type error in unary ops: `!` on number, `-` on string
- Array index out of bounds
- String index out of bounds
- Object field not found
- Calling a non-function value

### 2c. Value Serialization Tests

`value_to_json` for all value types:
- Null, Boolean (true/false), Number (integer, float, NaN edge case)
- String (plain, with escapes)
- Array (empty, nested)
- Object (empty, with hidden fields excluded)
- Circular reference detection

---

## File 3: compiler.rs (46% → 85%)

### 3a. Expand `compiler_integration_test.rs`

Pipeline tests (compile only, inspect bytecode) for uncovered compile paths:
- Object comprehension compilation
- `assert` expression compilation
- `super` field access compilation
- `import`/`importstr`/`importbin` compilation
- Function parameter defaults compilation
- `tailstrict` call compilation
- `$` (dollar) root reference compilation
- Object field visibility (`::` hidden, `:::` forced)
- `+:` field override syntax

### 3b. Inline Unit Tests

Compiler error paths:
- Duplicate parameter names in function definition
- Using `super` outside an object
- Using `self` outside an object
- Invalid `assert` syntax
- Undefined variable reference

---

## File 4: chunk.rs (47.8% → 85%)

Expand inline `mod tests`:

- **Disassembler:** test `disassemble()` output for every `Opcode` variant — each opcode should produce a non-empty disassembly string
- **Large operands:** `write_opcode_u16` and `write_opcode_u32` with values at boundary (e.g., 256, 65535, 65536)
- **Constant pool deduplication:** adding the same float constant twice returns the same index
- **Span tracking:** `get_span(offset)` returns correct span for written opcodes
- **Chunk serialization round-trip:** serialize to bytes and deserialize back, assert equality

---

## File 5: scanner.rs (64.6% → 85%)

Expand inline `mod tests` (smallest gap, ~135 lines):

- **Text blocks (`|||`):** basic multiline, `|||-` strip-newline variant, indentation stripping, mismatched indentation error, missing newline after `|||` error, unterminated text block error
- **Verbatim strings:** `@"foo""bar"` (doubled-quote escape), `@'it''s'`
- **Unicode escapes:** `\uXXXX` basic, surrogate pair `\uD800\uDC00`, unpaired high surrogate error, invalid low surrogate error, invalid hex digits error
- **Block comments:** unterminated block comment error
- **`is_incomplete_input()`:** returns true for unterminated/EOF messages, false for others
- **`into_report()`:** chained `ScanError` with cause produces report with both labels

---

## Success Criteria

- `bazel coverage //...` reports ≥ 85% line coverage for each of the five files
- All existing tests continue to pass (`bazel test //...`)
- No new clippy warnings (`bazel build --config=clippy //...`)
- Format check passes (`bazel build --config=rustfmt //...`)
