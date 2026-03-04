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
