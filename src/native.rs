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
