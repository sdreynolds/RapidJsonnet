// Copyright 2026 Scott Reynolds
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use chunk::{FunctionIndex, NativeFuncId, OwnedChunk, SpanRunLength, StringIndex, Value};
use fory::{Fory, ForyObject};
use memory_manager::MemoryManager;
use std::collections::HashMap;

// ── Serializable types ──────────────────────────────────────────────────────

/// Top-level container for a serialized compiled program.
#[derive(ForyObject, Debug, PartialEq)]
pub struct SerializedProgram {
    pub version: i32,
    pub source_id: String,
    pub string_table: Vec<String>,
    pub function_table: Vec<SerializedFunction>,
}

/// A serialized function with its bytecode chunk.
/// Index 0 in function_table is always the top-level script.
#[derive(ForyObject, Debug, PartialEq)]
pub struct SerializedFunction {
    /// Index into string_table for the function name, or -1 for anonymous/top-level.
    pub name: i32,
    pub arity: i32,
    pub upvalue_count: i32,
    pub chunk: SerializedChunk,
}

/// Serialized bytecode chunk (mirrors OwnedChunk).
#[derive(ForyObject, Debug, PartialEq)]
pub struct SerializedChunk {
    pub source_id: String,
    pub code: Vec<i32>,
    pub spans: Vec<SerializedSpan>,
    pub constants: Vec<SerializedValue>,
}

/// Serialized span (replaces SpanRunLength which uses Range<usize>).
#[derive(ForyObject, Debug, PartialEq)]
pub struct SerializedSpan {
    pub span_start: i64,
    pub span_end: i64,
    pub repeated_values: i64,
}

/// Serialized constant value. Uses a flat struct with a tag discriminant
/// because Fory enum support may be unreliable for data-carrying variants.
#[derive(ForyObject, Debug, PartialEq)]
pub struct SerializedValue {
    /// 0=Null, 1=Boolean, 2=Number, 3=StringRef, 4=FunctionRef, 5=NativeFunction
    pub tag: i32,
    pub bool_val: bool,
    pub number_val: f64,
    /// For StringRef/FunctionRef: index into respective table.
    /// For NativeFunction: NativeFuncId discriminant.
    pub index_val: i32,
}

// ── Tag constants ───────────────────────────────────────────────────────────

const TAG_NULL: i32 = 0;
const TAG_BOOLEAN: i32 = 1;
const TAG_NUMBER: i32 = 2;
const TAG_STRING_REF: i32 = 3;
const TAG_FUNCTION_REF: i32 = 4;
const TAG_NATIVE_FUNCTION: i32 = 5;

// ── Serialize direction ─────────────────────────────────────────────────────

struct SerializeContext<'a> {
    mm: &'a MemoryManager,
    string_table: Vec<String>,
    string_map: HashMap<StringIndex, i32>,
    function_table: Vec<SerializedFunction>,
    function_map: HashMap<FunctionIndex, i32>,
}

impl<'a> SerializeContext<'a> {
    fn new(mm: &'a MemoryManager) -> Self {
        Self {
            mm,
            string_table: Vec::new(),
            string_map: HashMap::new(),
            function_table: Vec::new(),
            function_map: HashMap::new(),
        }
    }

    /// Ensure a string is in the table, returning its index.
    fn intern_string(&mut self, idx: StringIndex) -> i32 {
        if let Some(&table_idx) = self.string_map.get(&idx) {
            return table_idx;
        }
        let content = self.mm.load_string(idx).to_string();
        let table_idx = self.string_table.len() as i32;
        self.string_table.push(content);
        self.string_map.insert(idx, table_idx);
        table_idx
    }

    /// Process a function, adding it and all reachable strings/functions to the tables.
    /// Returns the function's index in function_table.
    fn process_function(&mut self, idx: FunctionIndex) -> i32 {
        if let Some(&table_idx) = self.function_map.get(&idx) {
            return table_idx;
        }

        // Reserve a slot (needed for potential forward references, though the graph is a DAG)
        let table_idx = self.function_table.len() as i32;
        self.function_map.insert(idx, table_idx);
        // Push a placeholder
        self.function_table.push(SerializedFunction {
            name: -1,
            arity: 0,
            upvalue_count: 0,
            chunk: SerializedChunk {
                source_id: String::new(),
                code: Vec::new(),
                spans: Vec::new(),
                constants: Vec::new(),
            },
        });

        let func = self.mm.load_function(idx);
        let name = func.name.map(|n| self.intern_string(n)).unwrap_or(-1);
        let arity = func.arity as i32;
        let upvalue_count = func.upvalue_count as i32;
        let chunk = self.serialize_chunk(&func.chunk);

        self.function_table[table_idx as usize] = SerializedFunction {
            name,
            arity,
            upvalue_count,
            chunk,
        };

        table_idx
    }

    fn serialize_chunk(&mut self, chunk: &OwnedChunk) -> SerializedChunk {
        let constants: Vec<SerializedValue> = chunk
            .constants
            .iter()
            .map(|v| self.serialize_value(v))
            .collect();

        let spans: Vec<SerializedSpan> = chunk
            .spans
            .iter()
            .map(|s| SerializedSpan {
                span_start: s.span.start as i64,
                span_end: s.span.end as i64,
                repeated_values: s.repeated_values as i64,
            })
            .collect();

        let code: Vec<i32> = chunk.code.iter().map(|&b| b as i32).collect();

        SerializedChunk {
            source_id: chunk.source_id.clone(),
            code,
            spans,
            constants,
        }
    }

    fn serialize_value(&mut self, value: &Value) -> SerializedValue {
        match value {
            Value::Null => SerializedValue {
                tag: TAG_NULL,
                bool_val: false,
                number_val: 0.0,
                index_val: 0,
            },
            Value::Boolean(b) => SerializedValue {
                tag: TAG_BOOLEAN,
                bool_val: *b,
                number_val: 0.0,
                index_val: 0,
            },
            Value::Number(n) => SerializedValue {
                tag: TAG_NUMBER,
                bool_val: false,
                number_val: *n,
                index_val: 0,
            },
            Value::String(idx) => {
                let table_idx = self.intern_string(*idx);
                SerializedValue {
                    tag: TAG_STRING_REF,
                    bool_val: false,
                    number_val: 0.0,
                    index_val: table_idx,
                }
            }
            Value::Function(idx) => {
                let table_idx = self.process_function(*idx);
                SerializedValue {
                    tag: TAG_FUNCTION_REF,
                    bool_val: false,
                    number_val: 0.0,
                    index_val: table_idx,
                }
            }
            Value::NativeFunction(nf) => SerializedValue {
                tag: TAG_NATIVE_FUNCTION,
                bool_val: false,
                number_val: 0.0,
                index_val: *nf as i32,
            },
            _ => panic!(
                "Unexpected value type in compile-time constants: {:?}",
                value
            ),
        }
    }
}

/// Serialize a compiled chunk and its associated memory manager state to bytes.
pub fn serialize_program(chunk: &chunk::Chunk, mm: &MemoryManager) -> Vec<u8> {
    let owned = chunk.clone().into_owned();
    let mut ctx = SerializeContext::new(mm);

    // Serialize the top-level chunk as function index 0
    let top_chunk = ctx.serialize_chunk(&owned);
    // Insert at position 0
    ctx.function_table.insert(
        0,
        SerializedFunction {
            name: -1,
            arity: 0,
            upvalue_count: 0,
            chunk: top_chunk,
        },
    );
    // Shift all function indices by 1 since we inserted at position 0
    // Update function_map values
    for val in ctx.function_map.values_mut() {
        *val += 1;
    }
    // Update all FunctionRef index_val in all chunks' constants
    for func in &mut ctx.function_table {
        for constant in &mut func.chunk.constants {
            if constant.tag == TAG_FUNCTION_REF {
                constant.index_val += 1;
            }
        }
    }

    let program = SerializedProgram {
        version: 1,
        source_id: owned.source_id,
        string_table: ctx.string_table,
        function_table: ctx.function_table,
    };

    let mut fory = Fory::default();
    fory.register::<SerializedProgram>(100).unwrap();
    fory.register::<SerializedFunction>(101).unwrap();
    fory.register::<SerializedChunk>(102).unwrap();
    fory.register::<SerializedSpan>(103).unwrap();
    fory.register::<SerializedValue>(104).unwrap();

    fory.serialize(&program).expect("Fory serialization failed")
}

// ── Deserialize direction ───────────────────────────────────────────────────

/// Deserialize a compiled program from bytes into the given MemoryManager.
/// Returns the top-level OwnedChunk ready for VM execution.
pub fn deserialize_program(bytes: &[u8], mm: &mut MemoryManager) -> OwnedChunk {
    let mut fory = Fory::default();
    fory.register::<SerializedProgram>(100).unwrap();
    fory.register::<SerializedFunction>(101).unwrap();
    fory.register::<SerializedChunk>(102).unwrap();
    fory.register::<SerializedSpan>(103).unwrap();
    fory.register::<SerializedValue>(104).unwrap();

    let program: SerializedProgram = fory
        .deserialize(bytes)
        .expect("Fory deserialization failed");

    assert_eq!(program.version, 1, "Unsupported serialized program version");

    // 1. Allocate all strings into the memory manager
    let string_indices: Vec<StringIndex> = program
        .string_table
        .iter()
        .map(|s| mm.allocate_string(s).index)
        .collect();

    // 2. Allocate functions in reverse order (leaves first, so dependencies are ready)
    let func_count = program.function_table.len();
    let mut function_indices: Vec<Option<FunctionIndex>> = vec![None; func_count];

    for i in (0..func_count).rev() {
        if i == 0 {
            // Skip index 0 — that's the top-level chunk, returned directly
            continue;
        }

        let serialized_func = &program.function_table[i];
        let chunk = deserialize_chunk(serialized_func, &string_indices, &function_indices);
        let name = if serialized_func.name >= 0 {
            Some(string_indices[serialized_func.name as usize])
        } else {
            None
        };

        let result = mm.allocate_function(
            name,
            serialized_func.arity as u8,
            serialized_func.upvalue_count as u8,
            chunk,
        );
        function_indices[i] = Some(result.index);
    }

    // 3. Build the top-level OwnedChunk (function 0)
    let top_func = &program.function_table[0];
    deserialize_chunk(top_func, &string_indices, &function_indices)
}

fn deserialize_chunk(
    serialized_func: &SerializedFunction,
    string_indices: &[StringIndex],
    function_indices: &[Option<FunctionIndex>],
) -> OwnedChunk {
    let sc = &serialized_func.chunk;

    let code: Vec<u8> = sc.code.iter().map(|&b| b as u8).collect();

    let spans: Vec<SpanRunLength> = sc
        .spans
        .iter()
        .map(|s| SpanRunLength {
            span: (s.span_start as usize)..(s.span_end as usize),
            repeated_values: s.repeated_values as usize,
        })
        .collect();

    let constants: Vec<Value> = sc
        .constants
        .iter()
        .map(|sv| deserialize_value(sv, string_indices, function_indices))
        .collect();

    OwnedChunk {
        source_id: sc.source_id.clone(),
        code,
        spans,
        constants,
    }
}

fn deserialize_value(
    sv: &SerializedValue,
    string_indices: &[StringIndex],
    function_indices: &[Option<FunctionIndex>],
) -> Value {
    match sv.tag {
        TAG_NULL => Value::Null,
        TAG_BOOLEAN => Value::Boolean(sv.bool_val),
        TAG_NUMBER => Value::Number(sv.number_val),
        TAG_STRING_REF => Value::String(string_indices[sv.index_val as usize]),
        TAG_FUNCTION_REF => Value::Function(function_indices[sv.index_val as usize].expect(
            "Function reference points to unallocated function — ordering error in deserialization",
        )),
        TAG_NATIVE_FUNCTION => {
            let nf = NativeFuncId::from_u16(sv.index_val as u16)
                .expect("Invalid NativeFuncId discriminant");
            Value::NativeFunction(nf)
        }
        _ => panic!("Unknown serialized value tag: {}", sv.tag),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chunk::{Chunk, Opcode};

    fn make_fory() -> Fory {
        let mut fory = Fory::default();
        fory.register::<SerializedProgram>(100).unwrap();
        fory.register::<SerializedFunction>(101).unwrap();
        fory.register::<SerializedChunk>(102).unwrap();
        fory.register::<SerializedSpan>(103).unwrap();
        fory.register::<SerializedValue>(104).unwrap();
        fory
    }

    #[test]
    fn test_fory_round_trip_types() {
        let program = SerializedProgram {
            version: 1,
            source_id: "test.jsonnet".to_string(),
            string_table: vec!["hello".to_string(), "world".to_string()],
            function_table: vec![SerializedFunction {
                name: -1,
                arity: 0,
                upvalue_count: 0,
                chunk: SerializedChunk {
                    source_id: "test.jsonnet".to_string(),
                    code: vec![1, 2, 3],
                    spans: vec![SerializedSpan {
                        span_start: 0,
                        span_end: 5,
                        repeated_values: 3,
                    }],
                    constants: vec![
                        SerializedValue {
                            tag: TAG_NULL,
                            bool_val: false,
                            number_val: 0.0,
                            index_val: 0,
                        },
                        SerializedValue {
                            tag: TAG_NUMBER,
                            bool_val: false,
                            number_val: 42.0,
                            index_val: 0,
                        },
                        SerializedValue {
                            tag: TAG_STRING_REF,
                            bool_val: false,
                            number_val: 0.0,
                            index_val: 0,
                        },
                    ],
                },
            }],
        };

        let fory = make_fory();
        let bytes = fory.serialize(&program).unwrap();
        let decoded: SerializedProgram = fory.deserialize(&bytes).unwrap();
        assert_eq!(program, decoded);
    }

    #[test]
    fn test_round_trip_simple_number() {
        let mut mm = MemoryManager::new();

        // Build a simple chunk: LoadConst(42.0) + Return
        let source_id = "test.jsonnet";
        let mut chunk = Chunk::new(source_id);
        chunk.constants.push(Value::Number(42.0));
        chunk.write_opcode_u16(Opcode::LoadConst, 0, 0..4);
        chunk.write_opcode(Opcode::Return, 4..5);

        let bytes = serialize_program(&chunk, &mm);
        let owned_chunk = deserialize_program(&bytes, &mut mm);

        assert_eq!(owned_chunk.source_id, source_id);
        assert_eq!(owned_chunk.constants.len(), 1);
        assert_eq!(owned_chunk.constants[0], Value::Number(42.0));
        assert_eq!(owned_chunk.code, chunk.code);
    }

    #[test]
    fn test_round_trip_with_strings() {
        let mut mm = MemoryManager::new();
        let hello_idx = mm.allocate_string("hello").index;
        let world_idx = mm.allocate_string("world").index;

        let source_id = "test.jsonnet";
        let mut chunk = Chunk::new(source_id);
        chunk.constants.push(Value::String(hello_idx));
        chunk.constants.push(Value::String(world_idx));
        chunk.write_opcode_u16(Opcode::LoadConst, 0, 0..4);
        chunk.write_opcode_u16(Opcode::LoadConst, 1, 4..8);
        chunk.write_opcode(Opcode::Return, 8..9);

        let bytes = serialize_program(&chunk, &mm);

        // Deserialize into a fresh memory manager
        let mut mm2 = MemoryManager::new();
        let owned_chunk = deserialize_program(&bytes, &mut mm2);

        assert_eq!(owned_chunk.constants.len(), 2);

        // The string indices will be different but should resolve to the same content
        if let Value::String(idx) = owned_chunk.constants[0] {
            assert_eq!(mm2.load_string(idx), "hello");
        } else {
            panic!("Expected String constant");
        }

        if let Value::String(idx) = owned_chunk.constants[1] {
            assert_eq!(mm2.load_string(idx), "world");
        } else {
            panic!("Expected String constant");
        }
    }

    #[test]
    fn test_round_trip_with_nested_function() {
        let mut mm = MemoryManager::new();
        let name_idx = mm.allocate_string("myFunc").index;

        // Create an inner function chunk
        let mut inner_chunk = Chunk::new("test.jsonnet");
        inner_chunk.constants.push(Value::Number(1.0));
        inner_chunk.write_opcode_u16(Opcode::LoadConst, 0, 0..3);
        inner_chunk.write_opcode(Opcode::Return, 3..4);

        let func_result = mm.allocate_function(Some(name_idx), 1, 0, inner_chunk.into_owned());
        let func_idx = func_result.index;

        // Top-level chunk references the inner function
        let source_id = "test.jsonnet";
        let mut top_chunk = Chunk::new(source_id);
        top_chunk.constants.push(Value::Function(func_idx));
        top_chunk.constants.push(Value::Number(42.0));
        top_chunk.write_opcode_u16(Opcode::LoadConst, 0, 0..3);
        top_chunk.write_opcode_u16(Opcode::LoadConst, 1, 3..6);
        top_chunk.write_opcode(Opcode::Return, 6..7);

        let bytes = serialize_program(&top_chunk, &mm);

        let mut mm2 = MemoryManager::new();
        let owned_chunk = deserialize_program(&bytes, &mut mm2);

        assert_eq!(owned_chunk.constants.len(), 2);

        // Check the function reference was reconstructed
        if let Value::Function(new_func_idx) = owned_chunk.constants[0] {
            let func = mm2.load_function(new_func_idx);
            assert_eq!(func.arity, 1);
            assert_eq!(func.upvalue_count, 0);
            if let Some(name) = func.name {
                assert_eq!(mm2.load_string(name), "myFunc");
            } else {
                panic!("Expected function name");
            }
            // Check inner chunk has the right constant
            assert_eq!(func.chunk.constants.len(), 1);
            assert_eq!(func.chunk.constants[0], Value::Number(1.0));
        } else {
            panic!("Expected Function constant");
        }

        assert_eq!(owned_chunk.constants[1], Value::Number(42.0));
    }

    #[test]
    fn test_round_trip_native_function() {
        let mm = MemoryManager::new();

        let source_id = "test.jsonnet";
        let mut chunk = Chunk::new(source_id);
        chunk
            .constants
            .push(Value::NativeFunction(NativeFuncId::Length));
        chunk
            .constants
            .push(Value::NativeFunction(NativeFuncId::Type));
        chunk.write_opcode(Opcode::Return, 0..1);

        let bytes = serialize_program(&chunk, &mm);

        let mut mm2 = MemoryManager::new();
        let owned_chunk = deserialize_program(&bytes, &mut mm2);

        assert_eq!(owned_chunk.constants.len(), 2);
        assert_eq!(
            owned_chunk.constants[0],
            Value::NativeFunction(NativeFuncId::Length)
        );
        assert_eq!(
            owned_chunk.constants[1],
            Value::NativeFunction(NativeFuncId::Type)
        );
    }

    #[test]
    fn test_string_dedup_on_deserialize() {
        let mut mm = MemoryManager::new();
        let idx = mm.allocate_string("shared").index;

        let source_id = "test.jsonnet";
        let mut chunk = Chunk::new(source_id);
        // Same string referenced twice
        chunk.constants.push(Value::String(idx));
        chunk.constants.push(Value::String(idx));
        chunk.write_opcode(Opcode::Return, 0..1);

        let bytes = serialize_program(&chunk, &mm);

        let mut mm2 = MemoryManager::new();
        let owned_chunk = deserialize_program(&bytes, &mut mm2);

        // Both constants should point to the same StringIndex (interning dedup)
        if let (Value::String(a), Value::String(b)) =
            (owned_chunk.constants[0], owned_chunk.constants[1])
        {
            assert_eq!(a, b, "Same string should be deduplicated to same index");
            assert_eq!(mm2.load_string(a), "shared");
        } else {
            panic!("Expected String constants");
        }
    }

    #[test]
    fn test_deserialize_into_existing_memory_manager() {
        let mut mm = MemoryManager::new();
        let pre_existing = mm.allocate_string("pre_existing").index;

        let mut compile_mm = MemoryManager::new();
        let hello_idx = compile_mm.allocate_string("hello").index;

        let source_id = "test.jsonnet";
        let mut chunk = Chunk::new(source_id);
        chunk.constants.push(Value::String(hello_idx));
        chunk.write_opcode(Opcode::Return, 0..1);

        let bytes = serialize_program(&chunk, &compile_mm);

        // Deserialize into mm which already has "pre_existing"
        let owned_chunk = deserialize_program(&bytes, &mut mm);

        // Pre-existing string should still be accessible
        assert_eq!(mm.load_string(pre_existing), "pre_existing");

        // New string should also be accessible
        if let Value::String(idx) = owned_chunk.constants[0] {
            assert_eq!(mm.load_string(idx), "hello");
        } else {
            panic!("Expected String constant");
        }
    }
}
