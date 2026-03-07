use chunk::{FieldVisibility, NativeFuncId, RuntimeError, Value};
use memory_manager::MemoryManager;
use std::ops::Range;

/// Dispatches a native function call
pub fn call_native(
    id: NativeFuncId,
    args: &[Value],
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    if args.len() != id.arity() as usize {
        return Err(RuntimeError {
            span,
            message: format!(
                "Native function 'std.{}' expected {} arguments, but got {}",
                id.name(),
                id.arity(),
                args.len()
            ),
            source_id,
        });
    }

    match id {
        NativeFuncId::Type => std_type(args[0], memory_manager, span),
        NativeFuncId::Length => std_length(args[0], memory_manager, span, source_id),
        NativeFuncId::Abs => std_abs(args[0], span, source_id),
        NativeFuncId::Codepoint => std_codepoint(args[0], memory_manager, span, source_id),
        NativeFuncId::Char => std_char(args[0], memory_manager, span, source_id),
        NativeFuncId::MakeArray => Err(RuntimeError {
            span,
            message: "std.makeArray must be handled specially by the VM".to_string(),
            source_id,
        }),
        NativeFuncId::ToString => std_to_string(args[0], memory_manager, span, source_id),
        NativeFuncId::Floor => std_floor(args[0], span, source_id),
        NativeFuncId::Ceil => std_ceil(args[0], span, source_id),
        NativeFuncId::Round => std_round(args[0], span, source_id),
        NativeFuncId::Min => std_min(args[0], args[1], span, source_id),
        NativeFuncId::Max => std_max(args[0], args[1], span, source_id),
        NativeFuncId::Sign => std_sign(args[0], span, source_id),
        NativeFuncId::IsArray => std_is_array(args[0]),
        NativeFuncId::IsBoolean => std_is_boolean(args[0]),
        NativeFuncId::IsNumber => std_is_number(args[0]),
        NativeFuncId::IsObject => std_is_object(args[0]),
        NativeFuncId::IsString => std_is_string(args[0]),
        NativeFuncId::IsNull => std_is_null(args[0]),
        NativeFuncId::IsFunction => std_is_function(args[0]),
        NativeFuncId::ObjectFields => std_object_fields(args[0], memory_manager, span, source_id),
        NativeFuncId::ObjectHas => {
            std_object_has(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::ObjectValues => std_object_values(args[0], memory_manager, span, source_id),
        NativeFuncId::Range => std_range(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::ParseInt => std_parse_int(args[0], memory_manager, span, source_id),
        NativeFuncId::ParseOctal => std_parse_octal(args[0], memory_manager, span, source_id),
        NativeFuncId::ParseHex => std_parse_hex(args[0], memory_manager, span, source_id),
        NativeFuncId::AsciiUpper => std_ascii_upper(args[0], memory_manager, span, source_id),
        NativeFuncId::AsciiLower => std_ascii_lower(args[0], memory_manager, span, source_id),
        NativeFuncId::Substr => {
            std_substr(args[0], args[1], args[2], memory_manager, span, source_id)
        }
        NativeFuncId::Split => std_split(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Join => std_join(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Lines => std_lines(args[0], memory_manager, span, source_id),
        NativeFuncId::StringChars => std_string_chars(args[0], memory_manager, span, source_id),
        NativeFuncId::FlattenArrays => std_flatten_arrays(args[0], memory_manager, span, source_id),
        NativeFuncId::Reverse => std_reverse(args[0], memory_manager, span, source_id),
        NativeFuncId::Member => std_member(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Count => std_count(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Find => std_find(args[0], args[1], memory_manager, span, source_id),
    }
}

/// std.codepoint(str): Returns the positive integer representing the unicode codepoint of the string
fn std_codepoint(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx);
            let mut chars = s.chars();

            if let Some(c) = chars.next() {
                if chars.next().is_none() {
                    return Ok(Value::Number(c as u32 as f64));
                }
            }

            Err(RuntimeError {
                span,
                message: format!("std.codepoint() expected string of length 1, got '{}'", s),
                source_id,
            })
        }
        _ => Err(RuntimeError {
            span,
            message: format!("std.codepoint() expected string, but got something else"),
            source_id,
        }),
    }
}

/// std.char(n): Returns a string containing a single character corresponding to the unicode codepoint n
fn std_char(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => {
            if n < 0.0 || n.fract() != 0.0 {
                return Err(RuntimeError {
                    span,
                    message: format!("std.char() expected a positive integer, got {}", n),
                    source_id,
                });
            }

            let codepoint = n as u32;
            match std::char::from_u32(codepoint) {
                Some(c) => {
                    let allocation = memory_manager.allocate_string(&c.to_string());
                    Ok(Value::String(allocation.index))
                }
                None => Err(RuntimeError {
                    span,
                    message: format!("std.char() invalid unicode codepoint {}", codepoint),
                    source_id,
                }),
            }
        }
        _ => Err(RuntimeError {
            span,
            message: format!("std.char() expected number, but got something else"),
            source_id,
        }),
    }
}

/// std.type(val): Returns a string representing the type of the value
fn std_type(
    val: Value,
    memory_manager: &mut MemoryManager,
    _span: Range<usize>,
) -> Result<Value, RuntimeError> {
    let type_str = match val {
        Value::Null => "null",
        Value::Boolean(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::Function(_) | Value::Closure(_) | Value::NativeFunction(_) => "function",
        Value::Import(_) => "import",
        Value::Binary(_) => "binary",
    };

    let allocation = memory_manager.allocate_string(type_str);
    Ok(Value::String(allocation.index))
}

/// std.length(val): Returns the length of an array, string, or number of fields in an object
fn std_length(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx);
            // Jsonnet length of string is number of characters (not bytes)
            Ok(Value::Number(s.chars().count() as f64))
        }
        Value::Array(a_idx) => {
            let a = memory_manager.load_array(a_idx);
            Ok(Value::Number(a.elements.len() as f64))
        }
        Value::Object(o_idx) => {
            let o = memory_manager.load_object(o_idx);
            // In Jsonnet, std.length(obj) is the number of visible fields
            Ok(Value::Number(o.len() as f64))
        }
        _ => Err(RuntimeError {
            span,
            message: format!(
                "std.length() expected string, array, or object, but got something else"
            ),
            source_id,
        }),
    }
}

/// std.abs(n): Returns the absolute value of a number
fn std_abs(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.abs())),
        _ => Err(RuntimeError {
            span,
            message: format!("std.abs() expected number, but got something else"),
            source_id,
        }),
    }
}

/// std.toString(a): Converts any value to a string representation
fn std_to_string(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let s = match val {
        Value::String(_) => {
            // Already a string — return unchanged
            return Ok(val);
        }
        Value::Null => "null".to_string(),
        Value::Boolean(true) => "true".to_string(),
        Value::Boolean(false) => "false".to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                format!("{}", n as i64)
            } else {
                format!("{}", n)
            }
        }
        Value::Array(_) | Value::Object(_) => {
            return Err(RuntimeError {
                span,
                message: "std.toString() on objects and arrays is not yet implemented".to_string(),
                source_id,
            });
        }
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.toString() cannot convert this value type to string".to_string(),
                source_id,
            });
        }
    };
    let allocation = memory_manager.allocate_string(&s);
    Ok(Value::String(allocation.index))
}

/// std.floor(x): Returns the floor of x
fn std_floor(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.floor())),
        _ => Err(RuntimeError {
            span,
            message: "std.floor() expected number, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.ceil(x): Returns the ceiling of x
fn std_ceil(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.ceil())),
        _ => Err(RuntimeError {
            span,
            message: "std.ceil() expected number, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.round(x): Returns x rounded to the nearest integer using floor(x + 0.5) per spec
fn std_round(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number((n + 0.5).floor())),
        _ => Err(RuntimeError {
            span,
            message: "std.round() expected number, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.min(a, b): Returns the lesser of two numbers
fn std_min(
    a: Value,
    b: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x.min(y))),
        _ => Err(RuntimeError {
            span,
            message: "std.min() expected two numbers".to_string(),
            source_id,
        }),
    }
}

/// std.max(a, b): Returns the greater of two numbers
fn std_max(
    a: Value,
    b: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x.max(y))),
        _ => Err(RuntimeError {
            span,
            message: "std.max() expected two numbers".to_string(),
            source_id,
        }),
    }
}

/// std.sign(n): Returns -1, 0, or 1 depending on the sign of n
fn std_sign(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => {
            let sign = if n < 0.0 {
                -1.0
            } else if n > 0.0 {
                1.0
            } else {
                0.0
            };
            Ok(Value::Number(sign))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.sign() expected number, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.isArray(v): Returns true if v is an array
fn std_is_array(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::Array(_))))
}

/// std.isBoolean(v): Returns true if v is a boolean
fn std_is_boolean(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::Boolean(_))))
}

/// std.isNumber(v): Returns true if v is a number
fn std_is_number(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::Number(_))))
}

/// std.isObject(v): Returns true if v is an object
fn std_is_object(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::Object(_))))
}

/// std.isString(v): Returns true if v is a string
fn std_is_string(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::String(_))))
}

/// std.isNull(v): Returns true if v is null
fn std_is_null(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(val, Value::Null)))
}

/// std.isFunction(v): Returns true if v is a function
fn std_is_function(val: Value) -> Result<Value, RuntimeError> {
    Ok(Value::Boolean(matches!(
        val,
        Value::Function(_) | Value::Closure(_) | Value::NativeFunction(_)
    )))
}

/// std.objectFields(o): Returns an array of visible field names of o, sorted lexicographically
fn std_object_fields(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Object(o_idx) => {
            // Collect visible key indices first (ends the immutable borrow of load_object)
            let obj = memory_manager.load_object(o_idx);
            let visible_keys: Vec<chunk::StringIndex> = obj
                .properties
                .iter()
                .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                .map(|(key, _)| *key)
                .collect();
            // Now load the string names
            let mut names: Vec<String> = visible_keys
                .iter()
                .map(|key| memory_manager.load_string(*key).to_string())
                .collect();
            names.sort();
            let elements: Vec<Value> = names
                .iter()
                .map(|name| {
                    let alloc = memory_manager.allocate_string(name);
                    Value::String(alloc.index)
                })
                .collect();
            let arr_alloc = memory_manager.allocate_array(elements);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.objectFields() expected object, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.objectHas(o, f): Returns true if object o has a visible field named f
fn std_object_has(
    obj_val: Value,
    field_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (obj_val, field_val) {
        (Value::Object(o_idx), Value::String(s_idx)) => {
            // Intern the target field name string index — if the key exists at all
            // in the string pool we can compare by StringIndex directly, otherwise
            // it definitely doesn't match any field key.
            let field_name = memory_manager.load_string(s_idx).to_string();
            let obj = memory_manager.load_object(o_idx);
            // Collect visible key indices to avoid holding immutable borrow while
            // calling load_string again.
            let visible_keys: Vec<chunk::StringIndex> = obj
                .properties
                .iter()
                .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                .map(|(key, _)| *key)
                .collect();
            let found = visible_keys
                .iter()
                .any(|key| memory_manager.load_string(*key) == field_name);
            Ok(Value::Boolean(found))
        }
        (Value::Object(_), _) => Err(RuntimeError {
            span,
            message: "std.objectHas() second argument must be a string".to_string(),
            source_id,
        }),
        _ => Err(RuntimeError {
            span,
            message: "std.objectHas() first argument must be an object".to_string(),
            source_id,
        }),
    }
}

/// std.objectValues(o): Returns an array of visible field values of o, sorted by key name
fn std_object_values(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Object(o_idx) => {
            // Collect visible (key_index, value) pairs first (ends immutable borrow)
            let obj = memory_manager.load_object(o_idx);
            let visible_pairs: Vec<(chunk::StringIndex, Value)> = obj
                .properties
                .iter()
                .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                .map(|(key, field)| (*key, field.value))
                .collect();
            // Load the key names for sorting
            let mut named_pairs: Vec<(String, Value)> = visible_pairs
                .iter()
                .map(|(key, val)| (memory_manager.load_string(*key).to_string(), *val))
                .collect();
            named_pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let elements: Vec<Value> = named_pairs.into_iter().map(|(_, v)| v).collect();
            let arr_alloc = memory_manager.allocate_array(elements);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.objectValues() expected object, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.range(from, to): Returns an array [from, from+1, ..., to] (inclusive)
fn std_range(
    from_val: Value,
    to_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (from_val, to_val) {
        (Value::Number(from), Value::Number(to)) => {
            let from_i = from as i64;
            let to_i = to as i64;
            let elements: Vec<Value> = if from_i > to_i {
                Vec::new()
            } else {
                (from_i..=to_i).map(|i| Value::Number(i as f64)).collect()
            };
            let arr_alloc = memory_manager.allocate_array(elements);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.range() expected two numbers".to_string(),
            source_id,
        }),
    }
}

/// std.parseInt(str): Parse a decimal integer string
fn std_parse_int(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).trim().to_string();
            match s.parse::<i64>() {
                Ok(n) => Ok(Value::Number(n as f64)),
                Err(_) => Err(RuntimeError {
                    span,
                    message: format!("std.parseInt() failed to parse '{}' as integer", s),
                    source_id,
                }),
            }
        }
        _ => Err(RuntimeError {
            span,
            message: "std.parseInt() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.parseOctal(str): Parse an octal integer string
fn std_parse_octal(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).trim().to_string();
            match i64::from_str_radix(&s, 8) {
                Ok(n) => Ok(Value::Number(n as f64)),
                Err(_) => Err(RuntimeError {
                    span,
                    message: format!("std.parseOctal() failed to parse '{}' as octal", s),
                    source_id,
                }),
            }
        }
        _ => Err(RuntimeError {
            span,
            message: "std.parseOctal() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.parseHex(str): Parse a hexadecimal integer string (case-insensitive)
fn std_parse_hex(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).trim().to_lowercase();
            match i64::from_str_radix(&s, 16) {
                Ok(n) => Ok(Value::Number(n as f64)),
                Err(_) => Err(RuntimeError {
                    span,
                    message: format!("std.parseHex() failed to parse '{}' as hex", s),
                    source_id,
                }),
            }
        }
        _ => Err(RuntimeError {
            span,
            message: "std.parseHex() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.asciiUpper(str): Returns str with all ASCII letters uppercased
fn std_ascii_upper(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let upper: String = memory_manager
                .load_string(s_idx)
                .chars()
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let alloc = memory_manager.allocate_string(&upper);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.asciiUpper() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.asciiLower(str): Returns str with all ASCII letters lowercased
fn std_ascii_lower(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let lower: String = memory_manager
                .load_string(s_idx)
                .chars()
                .map(|c| c.to_ascii_lowercase())
                .collect();
            let alloc = memory_manager.allocate_string(&lower);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.asciiLower() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// Helper: compare two Values for equality, loading string contents when needed
fn values_equal(a: Value, b: Value, mm: &MemoryManager) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => mm.load_string(x) == mm.load_string(y),
        _ => a == b,
    }
}

/// std.substr(str, from, len): Returns a substring of str starting at from with length len
fn std_substr(
    str_val: Value,
    from_val: Value,
    len_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, from_n, len_n) = match (str_val, from_val, len_val) {
        (Value::String(s), Value::Number(f), Value::Number(l)) => (s, f, l),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.substr() expects (string, number, number)".to_string(),
                source_id,
            });
        }
    };
    if from_n < 0.0 || from_n.fract() != 0.0 {
        return Err(RuntimeError {
            span,
            message: format!(
                "std.substr() 'from' must be a non-negative integer, got {}",
                from_n
            ),
            source_id,
        });
    }
    if len_n < 0.0 || len_n.fract() != 0.0 {
        return Err(RuntimeError {
            span,
            message: format!(
                "std.substr() 'len' must be a non-negative integer, got {}",
                len_n
            ),
            source_id,
        });
    }
    let from = from_n as usize;
    let len = len_n as usize;
    let chars: Vec<char> = memory_manager.load_string(s_idx).chars().collect();
    let end = (from + len).min(chars.len());
    let result: String = chars[from.min(chars.len())..end].iter().collect();
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

/// std.split(str, c): Splits str on all occurrences of c, returning an array of strings
fn std_split(
    str_val: Value,
    c_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx) = match (str_val, c_val) {
        (Value::String(s), Value::String(c)) => (s, c),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.split() expects (string, string)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let c = memory_manager.load_string(c_idx).to_string();
    let parts: Vec<String> = s.split(c.as_str()).map(|p| p.to_string()).collect();
    let elements: Vec<Value> = parts
        .iter()
        .map(|p| {
            let alloc = memory_manager.allocate_string(p);
            Value::String(alloc.index)
        })
        .collect();
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

/// std.join(sep, arr): Joins an array of strings with sep, or interleaves sep array between sub-arrays
fn std_join(
    sep_val: Value,
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.join() second argument must be an array".to_string(),
                source_id,
            });
        }
    };

    match sep_val {
        Value::String(sep_idx) => {
            // String mode: join array of strings with separator
            let sep = memory_manager.load_string(sep_idx).to_string();
            let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
            let mut parts: Vec<String> = Vec::with_capacity(elements.len());
            for elem in &elements {
                match elem {
                    Value::String(s_idx) => {
                        parts.push(memory_manager.load_string(*s_idx).to_string());
                    }
                    _ => {
                        return Err(RuntimeError {
                            span,
                            message: "std.join() with string separator requires array of strings"
                                .to_string(),
                            source_id,
                        });
                    }
                }
            }
            let result = parts.join(&sep);
            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
        Value::Array(sep_arr_idx) => {
            // Array mode: interleave sep array between sub-arrays
            let sep_elements: Vec<Value> = memory_manager.load_array(sep_arr_idx).elements.clone();
            let outer_elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
            let mut result: Vec<Value> = Vec::new();
            for (i, elem) in outer_elements.iter().enumerate() {
                match elem {
                    Value::Array(sub_idx) => {
                        let sub_elements: Vec<Value> =
                            memory_manager.load_array(*sub_idx).elements.clone();
                        result.extend(sub_elements);
                        if i + 1 < outer_elements.len() {
                            result.extend(sep_elements.clone());
                        }
                    }
                    _ => {
                        return Err(RuntimeError {
                            span,
                            message: "std.join() with array separator requires array of arrays"
                                .to_string(),
                            source_id,
                        });
                    }
                }
            }
            let arr_alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.join() first argument must be a string or array".to_string(),
            source_id,
        }),
    }
}

/// std.lines(arr): Concatenates an array of strings, each followed by a newline
fn std_lines(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.lines() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut result = String::new();
    for elem in &elements {
        match elem {
            Value::String(s_idx) => {
                result.push_str(memory_manager.load_string(*s_idx));
                result.push('\n');
            }
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.lines() expected array of strings".to_string(),
                    source_id,
                });
            }
        }
    }
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

/// std.stringChars(str): Returns an array of single-character strings
fn std_string_chars(
    str_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let s_idx = match str_val {
        Value::String(s) => s,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.stringChars() expected string, but got something else".to_string(),
                source_id,
            });
        }
    };
    let chars: Vec<char> = memory_manager.load_string(s_idx).chars().collect();
    let elements: Vec<Value> = chars
        .iter()
        .map(|c| {
            let s = c.to_string();
            let alloc = memory_manager.allocate_string(&s);
            Value::String(alloc.index)
        })
        .collect();
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

/// std.flattenArrays(arr): Flattens one level of nested arrays into a single array
fn std_flatten_arrays(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.flattenArrays() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let outer: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut result: Vec<Value> = Vec::new();
    for elem in &outer {
        match elem {
            Value::Array(sub_idx) => {
                let sub: Vec<Value> = memory_manager.load_array(*sub_idx).elements.clone();
                result.extend(sub);
            }
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.flattenArrays() expected array of arrays".to_string(),
                    source_id,
                });
            }
        }
    }
    let arr_alloc = memory_manager.allocate_array(result);
    Ok(Value::Array(arr_alloc.index))
}

/// std.reverse(arr): Returns a new array with elements in reverse order
fn std_reverse(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.reverse() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let mut elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    elements.reverse();
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

/// std.member(arr, x): Returns true if x is in arr (array) or x is a substring of arr (string)
fn std_member(
    arr_val: Value,
    x_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match arr_val {
        Value::Array(arr_idx) => {
            let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
            let found = elements
                .iter()
                .any(|elem| values_equal(*elem, x_val, memory_manager));
            Ok(Value::Boolean(found))
        }
        Value::String(s_idx) => {
            let needle_idx = match x_val {
                Value::String(n) => n,
                _ => {
                    return Err(RuntimeError {
                        span,
                        message: "std.member() with string haystack requires string needle"
                            .to_string(),
                        source_id,
                    });
                }
            };
            let haystack = memory_manager.load_string(s_idx).to_string();
            let needle = memory_manager.load_string(needle_idx).to_string();
            Ok(Value::Boolean(haystack.contains(needle.as_str())))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.member() first argument must be an array or string".to_string(),
            source_id,
        }),
    }
}

/// std.count(arr, x): Returns the number of elements in arr equal to x
fn std_count(
    arr_val: Value,
    x_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.count() expected array as first argument".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let count = elements
        .iter()
        .filter(|elem| values_equal(**elem, x_val, memory_manager))
        .count();
    Ok(Value::Number(count as f64))
}

/// std.find(value, arr): Returns array of indices where arr[i] == value
fn std_find(
    value_val: Value,
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.find() expected array as second argument".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let indices: Vec<Value> = elements
        .iter()
        .enumerate()
        .filter(|(_, elem)| values_equal(**elem, value_val, memory_manager))
        .map(|(i, _)| Value::Number(i as f64))
        .collect();
    let arr_alloc = memory_manager.allocate_array(indices);
    Ok(Value::Array(arr_alloc.index))
}
