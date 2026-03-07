use chunk::{NativeFuncId, RuntimeError, Value};
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
