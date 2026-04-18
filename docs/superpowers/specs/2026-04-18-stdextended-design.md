# Design: `stdExtended` Namespace

**Date:** 2026-04-18  
**Status:** Approved

## Background

RapidJsonnet implements the full Jsonnet standard library (`std`) plus 21 extension functions that are not part of the upstream Jsonnet spec. Currently all 21 extensions are exposed under the `std` namespace alongside the standard functions, making it impossible to tell which functions are spec-compliant and which are RapidJsonnet-specific.

The goal is to move the 21 extension functions into a new `stdExtended` namespace, keeping `std` aligned with the upstream Jsonnet spec.

## Extension Functions Being Moved

The following 21 `NativeFuncId` variants move from `std` to `stdExtended`:

| Function | NativeFuncId |
|---|---|
| `parseFloat` | `ParseFloat` |
| `gcd` | `Gcd` |
| `lcm` | `Lcm` |
| `indent` | `Indent` |
| `chunk` | `Chunk` |
| `zip` | `Zip` |
| `unzip` | `Unzip` |
| `objectFromPairs` | `ObjectFromPairs` |
| `pick` | `Pick` |
| `omit` | `Omit` |
| `sortBy` | `SortBy` |
| `countBy` | `CountBy` |
| `uniqBy` | `UniqBy` |
| `toPairs` | `ToPairs` |
| `minBy` | `MinBy` |
| `maxBy` | `MaxBy` |
| `product` | `Product` |
| `groupBy` | `GroupBy` |
| `mapKeys` | `MapKeys` |
| `filterObject` | `FilterObject` |
| `objectFlatten` | `ObjectFlatten` |

The `NativeFuncId` variants themselves are unchanged — only which namespace exposes them changes.

## Architecture

`stdExtended` mirrors `std` exactly in the compiler and VM pipeline. No new concepts are introduced.

### `chunk.rs`

**Split `from_name()` into two functions:**
- `from_std_name(name: &str) -> Option<NativeFuncId>` — standard functions only; returns `None` for the 21 extensions
- `from_extended_name(name: &str) -> Option<NativeFuncId>` — the 21 extensions only

**Split `all_with_names()` into two functions:**
- `all_std_with_names() -> &'static [(&'static str, NativeFuncId)]` — standard functions only
- `all_extended_with_names() -> &'static [(&'static str, NativeFuncId)]` — the 21 extensions only

**New opcode:**
```
LoadStdExtended = 109   // next after LoadStd = 108
```

### `compiler.rs`

**New `ExpressionType` variant:**
```rust
StdExtendedNamespace,
```

**Identifier resolution** — new branch parallel to `"std"`:
```rust
} else if name_clone == "stdExtended" {
    self.emit_opcode(Opcode::LoadStdExtended, span);
    self.push_type(ExpressionType::StdExtendedNamespace);
}
```

**Dot handler** — new arm parallel to the `StdNamespace` arm:
- Detect `StdExtendedNamespace` on type stack top
- Pop type, emit `Pop`
- Look up name via `NativeFuncId::from_extended_name()`
- On success: emit `LoadConst` with `Value::NativeFunction(id)`, push `ExpressionType::NativeFunction(id)`
- On failure: compile error `"Native function 'stdExtended.{name}' not found"`

**`StdNamespace` dot handler update:** Switch lookup from `from_name()` to `from_std_name()`. This makes `std.mapKeys` etc. produce the existing "not found" compile error with no special hint.

### `virtual_machine.rs`

**New cached field on the VM struct:**
```rust
std_extended_object: Option<Value>,
```

**New opcode handler:**
```rust
Opcode::LoadStdExtended => {
    self.advance_pc();
    let obj = self.get_or_create_std_extended_object();
    self.push(obj)?;
}
```

**New builder method** — creates an object with only the 21 extension functions as hidden fields, caches it, and registers it as a GC root. Mirrors `get_or_create_std_object()` exactly.

**`get_or_create_std_object()` update:** Switch from `all_with_names()` to `all_std_with_names()` — one line.

### `native.rs`

Error message strings inside the 21 extension implementations are updated from `"std.foo: ..."` to `"stdExtended.foo: ..."` for consistency.

## Call Site Migration

All call sites are migrated from `std.foo` to `stdExtended.foo` for the 21 extension functions. This includes:

- Inline test strings in `src/native.rs`
- Inline test strings in `src/virtual_machine.rs`
- Any `.jsonnet` files in `end2end/`

The rename is purely textual; no logic changes.

## Testing

- **Existing tests** pass after call site migration (same coverage, new namespace)
- **New compile error tests** confirm `std.mapKeys`, `std.groupBy`, and a representative sample of the other 19 now produce compile errors
- **New first-class value tests** confirm:
  - `local e = stdExtended; e.mapKeys(...)` resolves correctly via runtime property lookup
  - `std.type(stdExtended)` returns `"object"`
