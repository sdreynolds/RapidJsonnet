use chunk::{FieldVisibility, NativeFuncId, RuntimeError, Value};
use memory_manager::{MemoryManager, ObjectField};
use std::collections::HashSet;
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
        NativeFuncId::Clamp => std_clamp(args[0], args[1], args[2], span, source_id),
        NativeFuncId::StartsWith => {
            std_starts_with(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::EndsWith => std_ends_with(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::FindSubstr => {
            std_find_substr(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::StrReplace => {
            std_str_replace(args[0], args[1], args[2], memory_manager, span, source_id)
        }
        NativeFuncId::IsEmpty => std_is_empty(args[0], memory_manager, span, source_id),
        NativeFuncId::All => std_all(args[0], memory_manager, span, source_id),
        NativeFuncId::Any => std_any(args[0], memory_manager, span, source_id),
        NativeFuncId::Sum => std_sum(args[0], memory_manager, span, source_id),
        NativeFuncId::AssertEqual => {
            std_assert_equal(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::Format => std_format(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::SplitLimit => {
            std_split_limit(args[0], args[1], args[2], memory_manager, span, source_id)
        }
        NativeFuncId::Repeat => std_repeat(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Slice => std_slice(
            args[0],
            args[1],
            args[2],
            args[3],
            memory_manager,
            span,
            source_id,
        ),
        NativeFuncId::Get => std_get(
            args[0],
            args[1],
            args[2],
            args[3],
            memory_manager,
            span,
            source_id,
        ),
        NativeFuncId::ObjectHasAll => {
            std_object_has_all(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::ObjectFieldsAll => {
            std_object_fields_all(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::EncodeUTF8 => std_encode_utf8(args[0], memory_manager, span, source_id),
        NativeFuncId::DecodeUTF8 => std_decode_utf8(args[0], memory_manager, span, source_id),
        NativeFuncId::Sort => std_sort(args[0], memory_manager, span, source_id),
        NativeFuncId::Uniq => std_uniq(args[0], memory_manager, span, source_id),
        NativeFuncId::SplitLimitR => {
            std_split_limit_r(args[0], args[1], args[2], memory_manager, span, source_id)
        }
        NativeFuncId::StripChars => {
            std_strip_chars(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::LstripChars => {
            std_lstrip_chars(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::RstripChars => {
            std_rstrip_chars(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::Trim => std_trim(args[0], memory_manager, span, source_id),
        NativeFuncId::ObjectKeysValues => {
            std_object_keys_values(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::Avg => std_avg(args[0], memory_manager, span, source_id),
        NativeFuncId::Remove => std_remove(args[0], args[1], memory_manager, span, source_id),
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
pub fn values_equal(a: Value, b: Value, mm: &MemoryManager) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Boolean(x), Value::Boolean(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => x == y,
        (Value::String(x), Value::String(y)) => mm.load_string(x) == mm.load_string(y),
        (Value::Array(x), Value::Array(y)) => {
            let ax = mm.load_array(x).elements.clone();
            let ay = mm.load_array(y).elements.clone();
            if ax.len() != ay.len() {
                return false;
            }
            ax.iter()
                .zip(ay.iter())
                .all(|(a, b)| values_equal(*a, *b, mm))
        }
        (Value::Object(x), Value::Object(y)) => {
            // Collect visible (key_string, value) pairs from each object, sorted by key name
            let get_visible = |obj_idx| -> Vec<(String, Value)> {
                let obj = mm.load_object(obj_idx);
                let visible: Vec<(chunk::StringIndex, Value)> = obj
                    .properties
                    .iter()
                    .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
                    .map(|(k, field)| (*k, field.value))
                    .collect();
                let mut named: Vec<(String, Value)> = visible
                    .into_iter()
                    .map(|(k, v)| (mm.load_string(k).to_string(), v))
                    .collect();
                named.sort_by(|a, b| a.0.cmp(&b.0));
                named
            };
            let ox = get_visible(x);
            let oy = get_visible(y);
            if ox.len() != oy.len() {
                return false;
            }
            ox.iter()
                .zip(oy.iter())
                .all(|((ka, va), (kb, vb))| ka == kb && values_equal(*va, *vb, mm))
        }
        _ => false,
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

/// std.clamp(x, minVal, maxVal): Clamps x to the range [minVal, maxVal]
fn std_clamp(
    x_val: Value,
    min_val: Value,
    max_val: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (x_val, min_val, max_val) {
        (Value::Number(x), Value::Number(lo), Value::Number(hi)) => {
            Ok(Value::Number(x.max(lo).min(hi)))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.clamp() expected three numbers".to_string(),
            source_id,
        }),
    }
}

/// std.startsWith(a, b): Returns true if string a starts with string b
fn std_starts_with(
    a_val: Value,
    b_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a_val, b_val) {
        (Value::String(a_idx), Value::String(b_idx)) => {
            let a = memory_manager.load_string(a_idx).to_string();
            let b = memory_manager.load_string(b_idx).to_string();
            Ok(Value::Boolean(a.starts_with(b.as_str())))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.startsWith() expected two strings".to_string(),
            source_id,
        }),
    }
}

/// std.endsWith(a, b): Returns true if string a ends with string b
fn std_ends_with(
    a_val: Value,
    b_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a_val, b_val) {
        (Value::String(a_idx), Value::String(b_idx)) => {
            let a = memory_manager.load_string(a_idx).to_string();
            let b = memory_manager.load_string(b_idx).to_string();
            Ok(Value::Boolean(a.ends_with(b.as_str())))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.endsWith() expected two strings".to_string(),
            source_id,
        }),
    }
}

/// std.findSubstr(pat, str): Returns array of codepoint indices of non-overlapping occurrences of pat in str
fn std_find_substr(
    pat_val: Value,
    str_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (pat_val, str_val) {
        (Value::String(pat_idx), Value::String(str_idx)) => {
            let pat = memory_manager.load_string(pat_idx).to_string();
            let s = memory_manager.load_string(str_idx).to_string();

            let indices: Vec<Value> = if pat.is_empty() {
                // Empty pattern matches every codepoint position (0..=len_in_chars)
                let char_count = s.chars().count();
                (0..=char_count).map(|i| Value::Number(i as f64)).collect()
            } else {
                // Collect chars for codepoint-indexed sliding window scan
                let s_chars: Vec<char> = s.chars().collect();
                let pat_chars: Vec<char> = pat.chars().collect();
                let pat_len = pat_chars.len();
                let s_len = s_chars.len();
                let mut result: Vec<Value> = Vec::new();
                let mut i = 0usize;
                while i + pat_len <= s_len {
                    if s_chars[i..i + pat_len] == pat_chars[..] {
                        result.push(Value::Number(i as f64));
                        i += pat_len; // non-overlapping: advance past match
                    } else {
                        i += 1;
                    }
                }
                result
            };

            let arr_alloc = memory_manager.allocate_array(indices);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.findSubstr() expected two strings".to_string(),
            source_id,
        }),
    }
}

/// std.strReplace(str, from, to): Replaces all non-overlapping occurrences of from with to in str
fn std_str_replace(
    str_val: Value,
    from_val: Value,
    to_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (str_val, from_val, to_val) {
        (Value::String(str_idx), Value::String(from_idx), Value::String(to_idx)) => {
            let s = memory_manager.load_string(str_idx).to_string();
            let from = memory_manager.load_string(from_idx).to_string();
            let to = memory_manager.load_string(to_idx).to_string();
            let result = s.replace(from.as_str(), to.as_str());
            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.strReplace() expected three strings".to_string(),
            source_id,
        }),
    }
}

/// std.isEmpty(str): Returns true if the string is empty
fn std_is_empty(
    str_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match str_val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx);
            Ok(Value::Boolean(s.is_empty()))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.isEmpty() expected string, but got something else".to_string(),
            source_id,
        }),
    }
}

/// std.all(arr): Returns true if all elements of arr are true
fn std_all(
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
                message: "std.all() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    for elem in &elements {
        match elem {
            Value::Boolean(b) => {
                if !b {
                    return Ok(Value::Boolean(false));
                }
            }
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.all() expected array of booleans".to_string(),
                    source_id,
                });
            }
        }
    }
    Ok(Value::Boolean(true))
}

/// std.any(arr): Returns true if any element of arr is true
fn std_any(
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
                message: "std.any() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    for elem in &elements {
        match elem {
            Value::Boolean(b) => {
                if *b {
                    return Ok(Value::Boolean(true));
                }
            }
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.any() expected array of booleans".to_string(),
                    source_id,
                });
            }
        }
    }
    Ok(Value::Boolean(false))
}

/// std.sum(arr): Returns the sum of all numbers in arr
fn std_sum(
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
                message: "std.sum() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut total = 0.0f64;
    for elem in &elements {
        match elem {
            Value::Number(n) => total += n,
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.sum() expected array of numbers".to_string(),
                    source_id,
                });
            }
        }
    }
    Ok(Value::Number(total))
}

/// std.assertEqual(a, b): Returns true if a equals b, otherwise errors
fn std_assert_equal(
    a_val: Value,
    b_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    if values_equal(a_val, b_val, memory_manager) {
        Ok(Value::Boolean(true))
    } else {
        let a_display = display_value(a_val, memory_manager);
        let b_display = display_value(b_val, memory_manager);
        Err(RuntimeError {
            span,
            message: format!(
                "Assertion failed: {} was not equal to {}",
                a_display, b_display
            ),
            source_id,
        })
    }
}

// ─── std.format ───────────────────────────────────────────────────────────────

/// Values supplied to a format string
pub enum FormatVals {
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
    Single(Value),
}

impl FormatVals {
    fn from_value(val: Value, mm: &MemoryManager) -> Self {
        match val {
            Value::Array(a_idx) => {
                let elems = mm.load_array(a_idx).elements.clone();
                FormatVals::Array(elems)
            }
            Value::Object(o_idx) => {
                let obj = mm.load_object(o_idx);
                let pairs: Vec<(String, Value)> = obj
                    .properties
                    .iter()
                    .map(|(k, f)| (mm.load_string(*k).to_string(), f.value))
                    .collect();
                FormatVals::Object(pairs)
            }
            other => FormatVals::Single(other),
        }
    }

    fn get_positional(
        &self,
        idx: usize,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        match self {
            FormatVals::Array(v) => v.get(idx).copied().ok_or_else(|| RuntimeError {
                span: span.clone(),
                message: format!(
                    "std.format: index {} out of range (array has {} elements)",
                    idx,
                    v.len()
                ),
                source_id: source_id.to_string(),
            }),
            FormatVals::Single(v) => {
                if idx == 0 {
                    Ok(*v)
                } else {
                    Err(RuntimeError {
                        span: span.clone(),
                        message: format!("std.format: index {} out of range (single value)", idx),
                        source_id: source_id.to_string(),
                    })
                }
            }
            FormatVals::Object(_) => Err(RuntimeError {
                span: span.clone(),
                message: "std.format: positional index used with object values".to_string(),
                source_id: source_id.to_string(),
            }),
        }
    }

    fn get_named(
        &self,
        key: &str,
        span: &Range<usize>,
        source_id: &str,
    ) -> Result<Value, RuntimeError> {
        match self {
            FormatVals::Object(pairs) => {
                for (k, v) in pairs {
                    if k == key {
                        return Ok(*v);
                    }
                }
                Err(RuntimeError {
                    span: span.clone(),
                    message: format!("std.format: key '{}' not found in object", key),
                    source_id: source_id.to_string(),
                })
            }
            _ => Err(RuntimeError {
                span: span.clone(),
                message: "std.format: named arg used but values is not an object".to_string(),
                source_id: source_id.to_string(),
            }),
        }
    }
}

fn value_to_format_string(val: Value, mm: &MemoryManager) -> String {
    match val {
        Value::String(idx) => mm.load_string(idx).to_string(),
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
        _ => "<value>".to_string(),
    }
}

fn apply_width_align(s: &str, flags: &str, width: usize) -> String {
    if width == 0 || s.len() >= width {
        return s.to_string();
    }
    let padding = width - s.len();
    if flags.contains('-') {
        format!("{}{}", s, " ".repeat(padding))
    } else {
        format!("{}{}", " ".repeat(padding), s)
    }
}

fn apply_numeric_format(s: &str, flags: &str, width: usize, zero_pad: bool) -> String {
    // s may start with '-'
    let (sign_part, num_part) = if s.starts_with('-') {
        ("-", &s[1..])
    } else if flags.contains('+') {
        ("+", s)
    } else if flags.contains(' ') {
        (" ", s)
    } else {
        ("", s)
    };

    let content = format!("{}{}", sign_part, num_part);
    if width == 0 || content.len() >= width {
        return content;
    }
    let padding = width - content.len();
    if flags.contains('-') {
        format!("{}{}", content, " ".repeat(padding))
    } else if zero_pad && !flags.contains('-') {
        // zero pad goes after sign
        format!("{}{}{}", sign_part, "0".repeat(padding), num_part)
    } else {
        format!("{}{}", " ".repeat(padding), content)
    }
}

/// Core format string implementation
pub fn format_string(
    fmt: &str,
    vals: &FormatVals,
    mm: &MemoryManager,
    span: &Range<usize>,
    source_id: &str,
) -> Result<String, RuntimeError> {
    let mut result = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut pos_idx: usize = 0;

    while i < chars.len() {
        if chars[i] != '%' {
            result.push(chars[i]);
            i += 1;
            continue;
        }
        i += 1; // skip '%'
        if i >= chars.len() {
            return Err(RuntimeError {
                span: span.clone(),
                message: "std.format: trailing '%' in format string".to_string(),
                source_id: source_id.to_string(),
            });
        }

        // Check for %% (literal percent)
        if chars[i] == '%' {
            result.push('%');
            i += 1;
            continue;
        }

        // Check for %(keyname) named argument
        let mut key_name: Option<String> = None;
        if chars[i] == '(' {
            i += 1;
            let mut key = String::new();
            while i < chars.len() && chars[i] != ')' {
                key.push(chars[i]);
                i += 1;
            }
            if i >= chars.len() {
                return Err(RuntimeError {
                    span: span.clone(),
                    message: "std.format: unclosed '(' in format specifier".to_string(),
                    source_id: source_id.to_string(),
                });
            }
            i += 1; // skip ')'
            key_name = Some(key);
        }

        // Parse flags: -, +, 0, space
        let mut flags = String::new();
        while i < chars.len() && "-+0 #".contains(chars[i]) {
            flags.push(chars[i]);
            i += 1;
        }

        // Parse width
        let mut width: usize = 0;
        while i < chars.len() && chars[i].is_ascii_digit() {
            width = width * 10 + (chars[i] as usize - '0' as usize);
            i += 1;
        }

        // Parse .precision
        let mut precision: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            let mut p: usize = 0;
            while i < chars.len() && chars[i].is_ascii_digit() {
                p = p * 10 + (chars[i] as usize - '0' as usize);
                i += 1;
            }
            precision = Some(p);
        }

        if i >= chars.len() {
            return Err(RuntimeError {
                span: span.clone(),
                message: "std.format: incomplete format specifier".to_string(),
                source_id: source_id.to_string(),
            });
        }

        let conv = chars[i];
        i += 1;

        // Get the value to format
        let val = if let Some(ref key) = key_name {
            vals.get_named(key, span, source_id)?
        } else {
            let v = vals.get_positional(pos_idx, span, source_id)?;
            pos_idx += 1;
            v
        };

        let zero_pad = flags.contains('0');

        let formatted = match conv {
            's' => {
                let s = value_to_format_string(val, mm);
                let s = if let Some(p) = precision {
                    // precision truncates string
                    s.chars().take(p).collect::<String>()
                } else {
                    s
                };
                apply_width_align(&s, &flags, width)
            }
            'd' | 'i' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: format!("std.format: %{} requires a number", conv),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let s = format!("{}", n as i64);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'o' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %o requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let i = n as i64;
                let s = if i < 0 {
                    format!("-{:o}", -i)
                } else {
                    format!("{:o}", i)
                };
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'x' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %x requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let s = format!("{:x}", n as i64);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'X' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %X requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let s = format!("{:X}", n as i64);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'f' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %f requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let p = precision.unwrap_or(6);
                let s = format!("{:.prec$}", n, prec = p);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'e' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %e requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let p = precision.unwrap_or(6);
                let s = format!("{:.prec$e}", n, prec = p);
                // Rust uses 'e+2' style; Python/Jsonnet uses 'e+02'
                let s = normalize_exp_notation(&s, false);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'E' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %E requires a number".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let p = precision.unwrap_or(6);
                let s = format!("{:.prec$e}", n, prec = p);
                let s = normalize_exp_notation(&s, true);
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'g' | 'G' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: format!("std.format: %{} requires a number", conv),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let p = precision.unwrap_or(6).max(1);
                let s = format_g(n, p, conv == 'G');
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'c' => {
                let n = match val {
                    Value::Number(n) => n as u32,
                    Value::String(s_idx) => {
                        let s = mm.load_string(s_idx);
                        match s.chars().next() {
                            Some(c) => c as u32,
                            None => {
                                return Err(RuntimeError {
                                    span: span.clone(),
                                    message: "std.format: %c requires non-empty string or number"
                                        .to_string(),
                                    source_id: source_id.to_string(),
                                });
                            }
                        }
                    }
                    _ => {
                        return Err(RuntimeError {
                            span: span.clone(),
                            message: "std.format: %c requires a number or string".to_string(),
                            source_id: source_id.to_string(),
                        });
                    }
                };
                let c = char::from_u32(n).unwrap_or('\u{FFFD}');
                apply_width_align(&c.to_string(), &flags, width)
            }
            _ => {
                return Err(RuntimeError {
                    span: span.clone(),
                    message: format!("std.format: unknown format specifier '%{}'", conv),
                    source_id: source_id.to_string(),
                });
            }
        };

        result.push_str(&formatted);
    }

    Ok(result)
}

/// Normalize Rust's scientific notation to match Python/Jsonnet style (e+02 not e+2)
fn normalize_exp_notation(s: &str, upper: bool) -> String {
    let e_char = if upper { 'E' } else { 'e' };
    if let Some(pos) = s.find('e') {
        let mantissa = &s[..pos];
        let exp_part = &s[pos + 1..];
        // exp_part is like "+2" or "-2" or "2"
        let (sign, digits) = if exp_part.starts_with('+') {
            ("+", &exp_part[1..])
        } else if exp_part.starts_with('-') {
            ("-", &exp_part[1..])
        } else {
            ("+", exp_part)
        };
        // Ensure at least 2 digit exponent
        let exp_num: i32 = digits.parse().unwrap_or(0);
        format!("{}{}{}{:02}", mantissa, e_char, sign, exp_num)
    } else {
        s.to_string()
    }
}

/// Format a float using %g/%G semantics
fn format_g(n: f64, prec: usize, upper: bool) -> String {
    // Use %e if exponent < -4 or >= prec, otherwise %f
    // prec is number of significant digits
    if n == 0.0 {
        return "0".to_string();
    }
    let exp = n.abs().log10().floor() as i32;
    if exp < -(4 as i32) || exp >= prec as i32 {
        // scientific notation with prec-1 decimal places
        let p = if prec > 0 { prec - 1 } else { 0 };
        let s = format!("{:.prec$e}", n, prec = p);
        // strip trailing zeros in mantissa
        let s = normalize_exp_notation(&s, upper);
        strip_trailing_zeros_e(&s)
    } else {
        // fixed notation — prec significant digits
        let decimal_places = if prec as i32 > exp + 1 {
            (prec as i32 - exp - 1) as usize
        } else {
            0
        };
        let s = format!("{:.prec$}", n, prec = decimal_places);
        // strip trailing zeros after decimal point
        if s.contains('.') {
            let s = s.trim_end_matches('0').trim_end_matches('.');
            s.to_string()
        } else {
            s
        }
    }
}

fn strip_trailing_zeros_e(s: &str) -> String {
    // Find the 'e' or 'E'
    let e_pos = s.find('e').or_else(|| s.find('E'));
    if let Some(pos) = e_pos {
        let mantissa = &s[..pos];
        let exp_part = &s[pos..];
        let mantissa = if mantissa.contains('.') {
            mantissa.trim_end_matches('0').trim_end_matches('.')
        } else {
            mantissa
        };
        format!("{}{}", mantissa, exp_part)
    } else {
        s.to_string()
    }
}

/// Public entry point for the % operator in the VM
pub fn std_format_public(
    fmt_val: Value,
    vals_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    std_format(fmt_val, vals_val, memory_manager, span, source_id)
}

/// std.format(str, vals): Python-style % string formatting
fn std_format(
    fmt_val: Value,
    vals_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let fmt_str = match fmt_val {
        Value::String(s_idx) => memory_manager.load_string(s_idx).to_string(),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.format() first argument must be a string".to_string(),
                source_id,
            });
        }
    };

    let vals = FormatVals::from_value(vals_val, memory_manager);
    let result = format_string(&fmt_str, &vals, memory_manager, &span, &source_id)?;
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

// ─── std.splitLimit ───────────────────────────────────────────────────────────

/// std.splitLimit(str, c, maxsplits): Like split but at most maxsplits splits
fn std_split_limit(
    str_val: Value,
    c_val: Value,
    max_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx, max) = match (str_val, c_val, max_val) {
        (Value::String(s), Value::String(c), Value::Number(m)) => (s, c, m as i64),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.splitLimit() expects (string, string, number)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let c = memory_manager.load_string(c_idx).to_string();
    let parts: Vec<String> = if max < 0 {
        s.split(c.as_str()).map(|p| p.to_string()).collect()
    } else {
        s.splitn(max as usize + 1, c.as_str())
            .map(|p| p.to_string())
            .collect()
    };
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

// ─── std.repeat ───────────────────────────────────────────────────────────────

/// std.repeat(what, count): Repeats an array or string count times
fn std_repeat(
    what_val: Value,
    count_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let count = match count_val {
        Value::Number(n) => {
            if n < 0.0 || n.fract() != 0.0 {
                return Err(RuntimeError {
                    span,
                    message: format!(
                        "std.repeat() count must be a non-negative integer, got {}",
                        n
                    ),
                    source_id,
                });
            }
            n as usize
        }
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.repeat() count must be a number".to_string(),
                source_id,
            });
        }
    };

    match what_val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let result = s.repeat(count);
            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
        Value::Array(a_idx) => {
            let elems = memory_manager.load_array(a_idx).elements.clone();
            let mut result: Vec<Value> = Vec::with_capacity(elems.len() * count);
            for _ in 0..count {
                result.extend_from_slice(&elems);
            }
            let arr_alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.repeat() first argument must be a string or array".to_string(),
            source_id,
        }),
    }
}

// ─── std.slice ────────────────────────────────────────────────────────────────

/// std.slice(indexable, index, end, step): Python-style slice
fn std_slice(
    indexable_val: Value,
    index_val: Value,
    end_val: Value,
    step_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match indexable_val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let chars: Vec<char> = s.chars().collect();
            let len = chars.len();
            let (start, end, step) =
                parse_slice_args(index_val, end_val, step_val, len, &span, &source_id)?;
            let mut result_chars = Vec::new();
            let mut i = start;
            while i < end {
                result_chars.push(chars[i]);
                i += step;
            }
            let result: String = result_chars.into_iter().collect();
            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
        Value::Array(a_idx) => {
            let elems = memory_manager.load_array(a_idx).elements.clone();
            let len = elems.len();
            let (start, end, step) =
                parse_slice_args(index_val, end_val, step_val, len, &span, &source_id)?;
            let mut result = Vec::new();
            let mut i = start;
            while i < end {
                result.push(elems[i]);
                i += step;
            }
            let arr_alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError {
            span,
            message: "std.slice() first argument must be a string or array".to_string(),
            source_id,
        }),
    }
}

fn parse_slice_args(
    index_val: Value,
    end_val: Value,
    step_val: Value,
    len: usize,
    span: &Range<usize>,
    source_id: &str,
) -> Result<(usize, usize, usize), RuntimeError> {
    let step = match step_val {
        Value::Null => 1usize,
        Value::Number(n) => {
            if n <= 0.0 || n.fract() != 0.0 {
                return Err(RuntimeError {
                    span: span.clone(),
                    message: format!("std.slice() step must be a positive integer, got {}", n),
                    source_id: source_id.to_string(),
                });
            }
            n as usize
        }
        _ => {
            return Err(RuntimeError {
                span: span.clone(),
                message: "std.slice() step must be a number or null".to_string(),
                source_id: source_id.to_string(),
            });
        }
    };

    let start = match index_val {
        Value::Null => 0usize,
        Value::Number(n) => {
            let idx = n as i64;
            if idx < 0 {
                let adjusted = len as i64 + idx;
                if adjusted < 0 { 0 } else { adjusted as usize }
            } else {
                (idx as usize).min(len)
            }
        }
        _ => {
            return Err(RuntimeError {
                span: span.clone(),
                message: "std.slice() index must be a number or null".to_string(),
                source_id: source_id.to_string(),
            });
        }
    };

    let end = match end_val {
        Value::Null => len,
        Value::Number(n) => {
            let idx = n as i64;
            if idx < 0 {
                let adjusted = len as i64 + idx;
                if adjusted < 0 { 0 } else { adjusted as usize }
            } else {
                (idx as usize).min(len)
            }
        }
        _ => {
            return Err(RuntimeError {
                span: span.clone(),
                message: "std.slice() end must be a number or null".to_string(),
                source_id: source_id.to_string(),
            });
        }
    };

    Ok((start, end, step))
}

// ─── std.get ──────────────────────────────────────────────────────────────────

/// std.get(o, f, default=null, inc_hidden=true): Get field from object with default
fn std_get(
    o_val: Value,
    f_val: Value,
    default_val: Value,
    inc_hidden_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let o_idx = match o_val {
        Value::Object(o) => o,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.get() first argument must be an object".to_string(),
                source_id,
            });
        }
    };

    let field_name = match f_val {
        Value::String(s_idx) => memory_manager.load_string(s_idx).to_string(),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.get() second argument must be a string".to_string(),
                source_id,
            });
        }
    };

    let inc_hidden = match inc_hidden_val {
        Value::Boolean(b) => b,
        Value::Null => true,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.get() fourth argument must be a boolean".to_string(),
                source_id,
            });
        }
    };

    let obj = memory_manager.load_object(o_idx);
    let found: Option<(Value, chunk::FieldVisibility)> = obj
        .properties
        .iter()
        .find(|(k, _)| memory_manager.load_string(**k) == field_name.as_str())
        .map(|(_, f)| (f.value, f.visibility));

    match found {
        Some((val, visibility)) => {
            if inc_hidden || visibility != chunk::FieldVisibility::Hidden {
                Ok(val)
            } else {
                Ok(default_val)
            }
        }
        None => Ok(default_val),
    }
}

// ─── std.objectHasAll ────────────────────────────────────────────────────────

/// std.objectHasAll(o, f): Returns true if object o has field f (including hidden fields)
fn std_object_has_all(
    obj_val: Value,
    field_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (obj_val, field_val) {
        (Value::Object(o_idx), Value::String(s_idx)) => {
            let field_name = memory_manager.load_string(s_idx).to_string();
            let obj = memory_manager.load_object(o_idx);
            let all_keys: Vec<chunk::StringIndex> = obj.properties.keys().copied().collect();
            let found = all_keys
                .iter()
                .any(|key| memory_manager.load_string(*key) == field_name);
            Ok(Value::Boolean(found))
        }
        (Value::Object(_), _) => Err(RuntimeError {
            span,
            message: "std.objectHasAll() second argument must be a string".to_string(),
            source_id,
        }),
        _ => Err(RuntimeError {
            span,
            message: "std.objectHasAll() first argument must be an object".to_string(),
            source_id,
        }),
    }
}

// ─── std.objectFieldsAll ─────────────────────────────────────────────────────

/// std.objectFieldsAll(o): Returns all field names (including hidden), sorted
fn std_object_fields_all(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Object(o_idx) => {
            let obj = memory_manager.load_object(o_idx);
            let all_keys: Vec<chunk::StringIndex> = obj.properties.keys().copied().collect();
            let mut names: Vec<String> = all_keys
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
            message: "std.objectFieldsAll() expected object, but got something else".to_string(),
            source_id,
        }),
    }
}

// ─── std.encodeUTF8 ──────────────────────────────────────────────────────────

/// std.encodeUTF8(str): Returns an array of byte values for the UTF-8 encoding of str
fn std_encode_utf8(
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
                message: "std.encodeUTF8() expected string, but got something else".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let elements: Vec<Value> = s.bytes().map(|b| Value::Number(b as f64)).collect();
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

// ─── std.decodeUTF8 ──────────────────────────────────────────────────────────

/// std.decodeUTF8(arr): Decodes an array of byte values as a UTF-8 string
fn std_decode_utf8(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let a_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.decodeUTF8() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(a_idx).elements.clone();
    let mut bytes: Vec<u8> = Vec::with_capacity(elements.len());
    for elem in &elements {
        match elem {
            Value::Number(n) => {
                if *n < 0.0 || *n > 255.0 || n.fract() != 0.0 {
                    return Err(RuntimeError {
                        span,
                        message: format!("std.decodeUTF8() byte value out of range: {}", n),
                        source_id,
                    });
                }
                bytes.push(*n as u8);
            }
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.decodeUTF8() array must contain numbers".to_string(),
                    source_id,
                });
            }
        }
    }
    match std::str::from_utf8(&bytes) {
        Ok(s) => {
            let alloc = memory_manager.allocate_string(s);
            Ok(Value::String(alloc.index))
        }
        Err(e) => Err(RuntimeError {
            span,
            message: format!("std.decodeUTF8() invalid UTF-8 sequence: {}", e),
            source_id,
        }),
    }
}

// ─── std.sort ────────────────────────────────────────────────────────────────

/// Compute a sort key for a value: (type_ord, numeric, string)
pub fn value_sort_key(val: Value, mm: &MemoryManager) -> (u8, f64, String) {
    match val {
        Value::Null => (0, 0.0, String::new()),
        Value::Boolean(false) => (1, 0.0, String::new()),
        Value::Boolean(true) => (2, 0.0, String::new()),
        Value::Number(n) => (3, n, String::new()),
        Value::String(s) => (4, 0.0, mm.load_string(s).to_string()),
        Value::Array(_) => (5, 0.0, String::new()),
        Value::Object(_) => (6, 0.0, String::new()),
        _ => (7, 0.0, String::new()),
    }
}

/// Compare two Values using sort-key ordering
pub fn compare_values(a: Value, b: Value, mm: &MemoryManager) -> std::cmp::Ordering {
    let ka = value_sort_key(a, mm);
    let kb = value_sort_key(b, mm);
    ka.0.cmp(&kb.0)
        .then_with(|| ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
        .then_with(|| ka.2.cmp(&kb.2))
}

/// std.sort(arr): Returns a sorted copy of arr using total type ordering
fn std_sort(
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
                message: "std.sort() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let mut elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    // Pre-compute sort keys to avoid repeated mm borrows during sort
    let keys: Vec<(u8, f64, String)> = elements
        .iter()
        .map(|v| value_sort_key(*v, memory_manager))
        .collect();
    let mut indexed: Vec<(usize, &(u8, f64, String))> = keys.iter().enumerate().collect();
    indexed.sort_by(|(_, a), (_, b)| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.2.cmp(&b.2))
    });
    let sorted: Vec<Value> = indexed.iter().map(|(i, _)| elements[*i]).collect();
    elements = sorted;
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

// ─── std.uniq ────────────────────────────────────────────────────────────────

/// std.uniq(arr): Removes consecutive duplicates from arr
fn std_uniq(
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
                message: "std.uniq() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut result: Vec<Value> = Vec::new();
    for elem in elements {
        if let Some(last) = result.last() {
            if values_equal(*last, elem, memory_manager) {
                continue;
            }
        }
        result.push(elem);
    }
    let arr_alloc = memory_manager.allocate_array(result);
    Ok(Value::Array(arr_alloc.index))
}

// ─── std.splitLimitR ─────────────────────────────────────────────────────────

/// std.splitLimitR(str, c, maxsplits): Like splitLimit but splits from the right
fn std_split_limit_r(
    str_val: Value,
    c_val: Value,
    max_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx, max) = match (str_val, c_val, max_val) {
        (Value::String(s), Value::String(c), Value::Number(m)) => (s, c, m as i64),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.splitLimitR() expects (string, string, number)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let c = memory_manager.load_string(c_idx).to_string();
    let parts: Vec<String> = if max < 0 {
        s.split(c.as_str()).map(|p| p.to_string()).collect()
    } else {
        // rsplitn splits from the right and returns in reverse order
        let mut rev_parts: Vec<String> = s
            .rsplitn(max as usize + 1, c.as_str())
            .map(|p| p.to_string())
            .collect();
        rev_parts.reverse();
        rev_parts
    };
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

// ─── std.stripChars / lstripChars / rstripChars / trim ───────────────────────

/// std.stripChars(str, chars): Strip chars from both ends of str
fn std_strip_chars(
    str_val: Value,
    chars_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx) = match (str_val, chars_val) {
        (Value::String(s), Value::String(c)) => (s, c),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.stripChars() expects (string, string)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let chars_str = memory_manager.load_string(c_idx).to_string();
    let char_set: HashSet<char> = chars_str.chars().collect();
    let chars: Vec<char> = s.chars().collect();
    let start = chars
        .iter()
        .position(|c| !char_set.contains(c))
        .unwrap_or(chars.len());
    let end = chars
        .iter()
        .rposition(|c| !char_set.contains(c))
        .map(|p| p + 1)
        .unwrap_or(0);
    let result: String = if start <= end {
        chars[start..end].iter().collect()
    } else {
        String::new()
    };
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

/// std.lstripChars(str, chars): Strip chars from the left end of str
fn std_lstrip_chars(
    str_val: Value,
    chars_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx) = match (str_val, chars_val) {
        (Value::String(s), Value::String(c)) => (s, c),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.lstripChars() expects (string, string)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let chars_str = memory_manager.load_string(c_idx).to_string();
    let char_set: HashSet<char> = chars_str.chars().collect();
    let chars: Vec<char> = s.chars().collect();
    let start = chars
        .iter()
        .position(|c| !char_set.contains(c))
        .unwrap_or(chars.len());
    let result: String = chars[start..].iter().collect();
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

/// std.rstripChars(str, chars): Strip chars from the right end of str
fn std_rstrip_chars(
    str_val: Value,
    chars_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let (s_idx, c_idx) = match (str_val, chars_val) {
        (Value::String(s), Value::String(c)) => (s, c),
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.rstripChars() expects (string, string)".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let chars_str = memory_manager.load_string(c_idx).to_string();
    let char_set: HashSet<char> = chars_str.chars().collect();
    let chars: Vec<char> = s.chars().collect();
    let end = chars
        .iter()
        .rposition(|c| !char_set.contains(c))
        .map(|p| p + 1)
        .unwrap_or(0);
    let result: String = chars[..end].iter().collect();
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

/// std.trim(str): Strip ASCII whitespace from both ends of str
fn std_trim(
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
                message: "std.trim() expected string, but got something else".to_string(),
                source_id,
            });
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let whitespace: HashSet<char> = [' ', '\t', '\n', '\r', '\x0B', '\x0C']
        .iter()
        .copied()
        .collect();
    let chars: Vec<char> = s.chars().collect();
    let start = chars
        .iter()
        .position(|c| !whitespace.contains(c))
        .unwrap_or(chars.len());
    let end = chars
        .iter()
        .rposition(|c| !whitespace.contains(c))
        .map(|p| p + 1)
        .unwrap_or(0);
    let result: String = if start <= end {
        chars[start..end].iter().collect()
    } else {
        String::new()
    };
    let alloc = memory_manager.allocate_string(&result);
    Ok(Value::String(alloc.index))
}

// ─── std.objectKeysValues ────────────────────────────────────────────────────

/// std.objectKeysValues(o): Returns [{key, value}] for each visible field, sorted by key
fn std_object_keys_values(
    obj_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let o_idx = match obj_val {
        Value::Object(o) => o,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.objectKeysValues() expected object, but got something else"
                    .to_string(),
                source_id,
            });
        }
    };
    // Collect visible (key_name, value) pairs
    let obj = memory_manager.load_object(o_idx);
    let visible_pairs: Vec<(chunk::StringIndex, Value)> = obj
        .properties
        .iter()
        .filter(|(_, field)| field.visibility != FieldVisibility::Hidden)
        .map(|(key, field)| (*key, field.value))
        .collect();
    let mut named_pairs: Vec<(String, Value)> = visible_pairs
        .iter()
        .map(|(key, val)| (memory_manager.load_string(*key).to_string(), *val))
        .collect();
    named_pairs.sort_by(|a, b| a.0.cmp(&b.0));

    // Intern the field-name strings "key" and "value" once
    let key_field_name = memory_manager.allocate_string("key").index;
    let value_field_name = memory_manager.allocate_string("value").index;

    let mut result_elements: Vec<Value> = Vec::with_capacity(named_pairs.len());
    for (name, val) in named_pairs {
        let name_str_idx = memory_manager.allocate_string(&name).index;
        let mut properties = std::collections::HashMap::new();
        properties.insert(
            key_field_name,
            ObjectField {
                value: Value::String(name_str_idx),
                super_obj: None,
                visibility: FieldVisibility::Visible,
            },
        );
        properties.insert(
            value_field_name,
            ObjectField {
                value: val,
                super_obj: None,
                visibility: FieldVisibility::Visible,
            },
        );
        let obj_alloc = memory_manager.allocate_object_with_properties(properties);
        result_elements.push(Value::Object(obj_alloc.index));
    }
    let arr_alloc = memory_manager.allocate_array(result_elements);
    Ok(Value::Array(arr_alloc.index))
}

// ─── std.avg ──────────────────────────────────────────────────────────────────

/// std.avg(arr): Returns the average of all numbers in arr
fn std_avg(
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
                message: "std.avg() expected array, but got something else".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    if elements.is_empty() {
        return Err(RuntimeError {
            span,
            message: "std.avg() array must be non-empty".to_string(),
            source_id,
        });
    }
    let mut total = 0.0f64;
    for elem in &elements {
        match elem {
            Value::Number(n) => total += n,
            _ => {
                return Err(RuntimeError {
                    span,
                    message: "std.avg() expected array of numbers".to_string(),
                    source_id,
                });
            }
        }
    }
    Ok(Value::Number(total / elements.len() as f64))
}

// ─── std.remove ───────────────────────────────────────────────────────────────

/// std.remove(arr, elem): Returns arr with the first occurrence of elem removed
fn std_remove(
    arr_val: Value,
    elem_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError {
                span,
                message: "std.remove() first argument must be an array".to_string(),
                source_id,
            });
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let first_match = elements
        .iter()
        .position(|v| values_equal(*v, elem_val, memory_manager));
    let result: Vec<Value> = match first_match {
        None => elements,
        Some(idx) => {
            let mut v = elements;
            v.remove(idx);
            v
        }
    };
    let arr_alloc = memory_manager.allocate_array(result);
    Ok(Value::Array(arr_alloc.index))
}

/// Format a value as a human-readable string for error messages (no allocation into mm)
fn display_value(val: Value, memory_manager: &MemoryManager) -> String {
    match val {
        Value::Null => "null".to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Number(n) => {
            if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
                format!("{}", n as i64)
            } else {
                format!("{}", n)
            }
        }
        Value::String(idx) => format!("\"{}\"", memory_manager.load_string(idx)),
        Value::Array(_) => "<array>".to_string(),
        Value::Object(_) => "<object>".to_string(),
        Value::Function(_) | Value::Closure(_) | Value::NativeFunction(_) => {
            "<function>".to_string()
        }
        _ => "<value>".to_string(),
    }
}
