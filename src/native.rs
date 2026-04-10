use chunk::{FieldVisibility, NativeFuncId, RuntimeError, StringIndex, Value};
use memory_manager::{MemoryManager, ObjectField};
use std::collections::HashSet;
use std::ops::Range;

/// Convert a string or array value to an array of values for set operations.
/// Strings are treated as arrays of single-character strings (sorted).
fn coerce_to_sorted_array(
    val: Value,
    func_name: &str,
    arg_name: &str,
    memory_manager: &mut MemoryManager,
    span: &Range<usize>,
    source_id: &str,
) -> Result<Vec<Value>, RuntimeError> {
    match val {
        Value::Array(i) => Ok(memory_manager.load_array(i).elements.clone()),
        Value::String(idx) => {
            let s = memory_manager.load_string(idx).to_string();
            let mut chars: Vec<String> = s.chars().map(|c| c.to_string()).collect();
            chars.sort();
            chars.dedup();
            let values: Vec<Value> = chars
                .into_iter()
                .map(|c| {
                    let interned = memory_manager.allocate_string(&c);
                    Value::String(interned.index)
                })
                .collect();
            Ok(values)
        }
        _ => Err(RuntimeError::new(
            span.clone(),
            format!("std.{}: {} must be an array or string", func_name, arg_name),
            source_id.to_string(),
        )),
    }
}

/// Dispatches a native function call
pub fn call_native(
    id: NativeFuncId,
    args: &[Value],
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    if args.len() != id.arity() as usize {
        return Err(RuntimeError::new(
            span,
            format!(
                "Native function 'std.{}' expected {} arguments, but got {}",
                id.name(),
                id.arity(),
                args.len()
            ),
            source_id,
        ));
    }

    match id {
        NativeFuncId::Type => std_type(args[0], memory_manager, span),
        NativeFuncId::Length => std_length(args[0], memory_manager, span, source_id),
        NativeFuncId::Abs => std_abs(args[0], span, source_id),
        NativeFuncId::Codepoint => std_codepoint(args[0], memory_manager, span, source_id),
        NativeFuncId::Char => std_char(args[0], memory_manager, span, source_id),
        NativeFuncId::MakeArray => Err(RuntimeError::new(
            span,
            "std.makeArray must be handled specially by the VM".to_string(),
            source_id,
        )),
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
        NativeFuncId::Sort => Err(RuntimeError::new(
            span,
            format!("std.sort must be handled by the VM"),
            source_id,
        )),
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
        NativeFuncId::Base64 => std_base64(args[0], memory_manager, span, source_id),
        NativeFuncId::Base64DecodeBytes => {
            std_base64_decode_bytes(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::EscapeStringJson => {
            std_escape_string_json(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::EscapeStringXml => {
            std_escape_string_xml(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::EscapeStringBash => {
            std_escape_string_bash(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::ParseFloat => std_parse_float(args[0], memory_manager, span, source_id),
        NativeFuncId::Pow => std_pow(args[0], args[1], span, source_id),
        NativeFuncId::Sqrt => std_sqrt(args[0], span, source_id),
        NativeFuncId::Exp => std_exp(args[0], span, source_id),
        NativeFuncId::Log => std_log(args[0], span, source_id),
        NativeFuncId::IsEven => std_is_even(args[0], span, source_id),
        NativeFuncId::IsOdd => std_is_odd(args[0], span, source_id),
        NativeFuncId::Contains => std_contains(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::ObjectValuesAll => {
            std_object_values_all(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::Sin => std_sin(args[0], span, source_id),
        NativeFuncId::Cos => std_cos(args[0], span, source_id),
        NativeFuncId::Tan => std_tan(args[0], span, source_id),
        NativeFuncId::Log2 => std_log2(args[0], span, source_id),
        NativeFuncId::Log10 => std_log10(args[0], span, source_id),
        NativeFuncId::Xor => std_xor(args[0], args[1], span, source_id),
        NativeFuncId::Xnor => std_xnor(args[0], args[1], span, source_id),
        NativeFuncId::ObjectKeysValuesAll => {
            std_object_keys_values_all(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::Asin => std_asin(args[0], span, source_id),
        NativeFuncId::Acos => std_acos(args[0], span, source_id),
        NativeFuncId::Atan => std_atan(args[0], span, source_id),
        NativeFuncId::Atan2 => std_atan2(args[0], args[1], span, source_id),
        NativeFuncId::IsInteger => std_is_integer(args[0], span, source_id),
        NativeFuncId::IsDecimal => std_is_decimal(args[0], span, source_id),
        NativeFuncId::ObjectRemoveKey => {
            std_object_remove_key(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::FlattenDeepArray => {
            std_flatten_deep_array(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::Deg2Rad => std_deg2rad(args[0], span, source_id),
        NativeFuncId::Rad2Deg => std_rad2deg(args[0], span, source_id),
        NativeFuncId::Hypot => std_hypot(args[0], args[1], span, source_id),
        NativeFuncId::RemoveAt => std_remove_at(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::EscapeStringDollars => {
            std_escape_string_dollars(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::EqualsIgnoreCase => {
            std_equals_ignore_case(args[0], args[1], memory_manager, span, source_id)
        }
        NativeFuncId::Trace => std_trace(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Base64Decode => {
            std_base64_decode_string(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::MinArray => std_min_array(args[0], memory_manager, span, source_id),
        NativeFuncId::MaxArray => std_max_array(args[0], memory_manager, span, source_id),
        NativeFuncId::DeepJoin => std_deep_join(args[0], memory_manager, span, source_id),
        NativeFuncId::ManifestJsonEx
        | NativeFuncId::ManifestJson
        | NativeFuncId::ManifestJsonMinified
        | NativeFuncId::Uniq
        | NativeFuncId::Prune
        | NativeFuncId::MergePatch
        | NativeFuncId::Set
        | NativeFuncId::SetUnion
        | NativeFuncId::ManifestIni
        | NativeFuncId::ManifestPython
        | NativeFuncId::ManifestPythonVars
        | NativeFuncId::ManifestYamlDoc
        | NativeFuncId::ManifestYamlStream
        | NativeFuncId::ManifestTomlEx
        | NativeFuncId::ParseYaml
        | NativeFuncId::ManifestXmlJsonml
        | NativeFuncId::ExtVar => Err(RuntimeError::new(
            span,
            format!("std.{} must be handled by the VM", id.name()),
            source_id,
        )),
        NativeFuncId::Sha256 => match args[0] {
            Value::String(idx) => {
                let s = memory_manager.load_string(idx).to_string();
                use sha2::Digest;
                let mut hasher = sha2::Sha256::new();
                hasher.update(s.as_bytes());
                let hex = format!("{:x}", hasher.finalize());
                let interned = memory_manager.allocate_string(&hex);
                Ok(Value::String(interned.index))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.sha256 expected string, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Sha1 => match args[0] {
            Value::String(idx) => {
                let s = memory_manager.load_string(idx).to_string();
                use sha1::Digest;
                let mut hasher = sha1::Sha1::new();
                hasher.update(s.as_bytes());
                let hex = format!("{:x}", hasher.finalize());
                let interned = memory_manager.allocate_string(&hex);
                Ok(Value::String(interned.index))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.sha1 expected string, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Sha512 => match args[0] {
            Value::String(idx) => {
                let s = memory_manager.load_string(idx).to_string();
                use sha2::Digest;
                let mut hasher = sha2::Sha512::new();
                hasher.update(s.as_bytes());
                let hex = format!("{:x}", hasher.finalize());
                let interned = memory_manager.allocate_string(&hex);
                Ok(Value::String(interned.index))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.sha512 expected string, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Sha3 => match args[0] {
            Value::String(idx) => {
                let s = memory_manager.load_string(idx).to_string();
                use sha3::Digest as Sha3Digest;
                let mut hasher = sha3::Sha3_256::new();
                hasher.update(s.as_bytes());
                let hex = format!("{:x}", hasher.finalize());
                let interned = memory_manager.allocate_string(&hex);
                Ok(Value::String(interned.index))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.sha3 expected string, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Map
        | NativeFuncId::Filter
        | NativeFuncId::Foldl
        | NativeFuncId::FlatMap
        | NativeFuncId::MapWithIndex
        | NativeFuncId::ParseJson
        | NativeFuncId::Foldr
        | NativeFuncId::MapWithKey
        | NativeFuncId::FilterMap
        | NativeFuncId::GroupBy
        | NativeFuncId::MapKeys
        | NativeFuncId::FilterObject
        | NativeFuncId::ObjectFlatten
        | NativeFuncId::SortBy
        | NativeFuncId::CountBy
        | NativeFuncId::UniqBy
        | NativeFuncId::MinBy
        | NativeFuncId::MaxBy
        | NativeFuncId::ToPairs => Err(RuntimeError::new(
            span,
            format!("std.{} must be handled by the VM", id.name()),
            source_id,
        )),
        NativeFuncId::Gcd => {
            let a = match args[0] {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as u64,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.gcd: expected non-negative integer".to_string(),
                        source_id,
                    ));
                }
            };
            let b = match args[1] {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as u64,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.gcd: expected non-negative integer".to_string(),
                        source_id,
                    ));
                }
            };
            fn gcd(mut x: u64, mut y: u64) -> u64 {
                while y != 0 {
                    let t = y;
                    y = x % y;
                    x = t;
                }
                x
            }
            Ok(Value::Number(gcd(a, b) as f64))
        }
        NativeFuncId::Lcm => {
            let a = match args[0] {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as u64,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.lcm: expected non-negative integer".to_string(),
                        source_id,
                    ));
                }
            };
            let b = match args[1] {
                Value::Number(n) if n >= 0.0 && n.fract() == 0.0 => n as u64,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.lcm: expected non-negative integer".to_string(),
                        source_id,
                    ));
                }
            };
            fn gcd(mut x: u64, mut y: u64) -> u64 {
                while y != 0 {
                    let t = y;
                    y = x % y;
                    x = t;
                }
                x
            }
            let g = gcd(a, b);
            let result = if g == 0 { 0u64 } else { a / g * b };
            Ok(Value::Number(result as f64))
        }
        NativeFuncId::Indent => {
            let s = match args[0] {
                Value::String(idx) => memory_manager.load_string(idx).to_string(),
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.indent: first argument must be a string".to_string(),
                        source_id,
                    ));
                }
            };
            let prefix = match args[1] {
                Value::String(idx) => memory_manager.load_string(idx).to_string(),
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.indent: second argument must be a string".to_string(),
                        source_id,
                    ));
                }
            };
            if s.is_empty() {
                let interned = memory_manager.allocate_string("");
                return Ok(Value::String(interned.index));
            }
            let trailing_newline = s.ends_with('\n');
            let content = if trailing_newline {
                &s[..s.len() - 1]
            } else {
                &s[..]
            };
            let indented: String = content
                .split('\n')
                .map(|line| format!("{}{}", prefix, line))
                .collect::<Vec<_>>()
                .join("\n");
            let result = if trailing_newline {
                indented + "\n"
            } else {
                indented
            };
            let interned = memory_manager.allocate_string(&result);
            Ok(Value::String(interned.index))
        }
        NativeFuncId::SetInter => {
            let a_val = args[0];
            let b_val = args[1];
            let a_elems = coerce_to_sorted_array(
                a_val,
                "setInter",
                "first argument",
                memory_manager,
                &span,
                &source_id,
            )?;
            let b_elems = coerce_to_sorted_array(
                b_val,
                "setInter",
                "second argument",
                memory_manager,
                &span,
                &source_id,
            )?;
            let mut result: Vec<Value> = Vec::new();
            let mut i = 0;
            let mut j = 0;
            while i < a_elems.len() && j < b_elems.len() {
                let cmp = compare_values(a_elems[i], b_elems[j], memory_manager);
                match cmp {
                    std::cmp::Ordering::Less => i += 1,
                    std::cmp::Ordering::Greater => j += 1,
                    std::cmp::Ordering::Equal => {
                        result.push(a_elems[i]);
                        i += 1;
                        j += 1;
                    }
                }
            }
            let alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(alloc.index))
        }
        NativeFuncId::SetDiff => {
            let a_val = args[0];
            let b_val = args[1];
            let a_elems = coerce_to_sorted_array(
                a_val,
                "setDiff",
                "first argument",
                memory_manager,
                &span,
                &source_id,
            )?;
            let b_elems = coerce_to_sorted_array(
                b_val,
                "setDiff",
                "second argument",
                memory_manager,
                &span,
                &source_id,
            )?;
            let mut result: Vec<Value> = Vec::new();
            let mut i = 0;
            let mut j = 0;
            while i < a_elems.len() {
                if j >= b_elems.len() {
                    result.push(a_elems[i]);
                    i += 1;
                } else {
                    let cmp = compare_values(a_elems[i], b_elems[j], memory_manager);
                    match cmp {
                        std::cmp::Ordering::Less => {
                            result.push(a_elems[i]);
                            i += 1;
                        }
                        std::cmp::Ordering::Greater => j += 1,
                        std::cmp::Ordering::Equal => {
                            i += 1;
                            j += 1;
                        }
                    }
                }
            }
            let alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(alloc.index))
        }
        NativeFuncId::SetMember => {
            let x_val = args[0];
            let arr_val = args[1];
            let arr_idx = match arr_val {
                Value::Array(i) => i,
                _ => {
                    return Err(RuntimeError::new(
                        span,
                        "std.setMember: second argument must be an array".to_string(),
                        source_id,
                    ));
                }
            };
            let elems = memory_manager.load_array(arr_idx).elements.clone();
            let mut lo = 0usize;
            let mut hi = elems.len();
            let mut found = false;
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let cmp = compare_values(x_val, elems[mid], memory_manager);
                match cmp {
                    std::cmp::Ordering::Equal => {
                        found = true;
                        break;
                    }
                    std::cmp::Ordering::Less => hi = mid,
                    std::cmp::Ordering::Greater => lo = mid + 1,
                }
            }
            Ok(Value::Boolean(found))
        }
        NativeFuncId::Mantissa => match args[0] {
            Value::Number(n) => {
                let (mantissa, _exp) = frexp(n);
                Ok(Value::Number(mantissa))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.mantissa expected number, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Exponent => match args[0] {
            Value::Number(n) => {
                let (_mantissa, exp) = frexp(n);
                Ok(Value::Number(exp as f64))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.exponent expected number, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Md5 => match args[0] {
            Value::String(idx) => {
                let s = memory_manager.load_string(idx).to_string();
                let digest = md5::compute(s.as_bytes());
                let hex = format!("{:032x}", digest);
                let interned = memory_manager.allocate_string(&hex);
                Ok(Value::String(interned.index))
            }
            _ => Err(RuntimeError::new(
                span,
                format!("std.md5 expected string, got {}", args[0].type_name()),
                source_id,
            )),
        },
        NativeFuncId::Chunk => std_chunk(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Zip => std_zip(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Unzip => std_unzip(args[0], memory_manager, span, source_id),
        NativeFuncId::ObjectFromPairs => {
            std_object_from_pairs(args[0], memory_manager, span, source_id)
        }
        NativeFuncId::Pick => std_pick(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Omit => std_omit(args[0], args[1], memory_manager, span, source_id),
        NativeFuncId::Product => std_product(args[0], memory_manager, span, source_id),
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

            Err(RuntimeError::new(
                span,
                format!("std.codepoint() expected string of length 1, got '{}'", s),
                source_id,
            ))
        }
        _ => Err(RuntimeError::new(
            span,
            format!("std.codepoint() expected string, but got something else"),
            source_id,
        )),
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
                return Err(RuntimeError::new(
                    span,
                    format!("std.char() expected a positive integer, got {}", n),
                    source_id,
                ));
            }

            let codepoint = n as u32;
            match std::char::from_u32(codepoint) {
                Some(c) => {
                    let allocation = memory_manager.allocate_string(&c.to_string());
                    Ok(Value::String(allocation.index))
                }
                None => Err(RuntimeError::new(
                    span,
                    format!("std.char() invalid unicode codepoint {}", codepoint),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            format!("std.char() expected number, but got something else"),
            source_id,
        )),
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
        Value::Uninitialized => "uninitialized",
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
            // Walk the full base_object chain; visible fields win on collision
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let count = fields
                .iter()
                .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
                .count();
            Ok(Value::Number(count as f64))
        }
        _ => Err(RuntimeError::new(
            span,
            format!("std.length() expected string, array, or object, but got something else"),
            source_id,
        )),
    }
}

/// std.abs(n): Returns the absolute value of a number
fn std_abs(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.abs())),
        _ => Err(RuntimeError::new(
            span,
            format!("std.abs() expected number, but got something else"),
            source_id,
        )),
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
            return Err(RuntimeError::new(
                span,
                "std.toString() on objects and arrays is not yet implemented".to_string(),
                source_id,
            ));
        }
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.toString() cannot convert this value type to string".to_string(),
                source_id,
            ));
        }
    };
    let allocation = memory_manager.allocate_string(&s);
    Ok(Value::String(allocation.index))
}

/// std.floor(x): Returns the floor of x
fn std_floor(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.floor())),
        _ => Err(RuntimeError::new(
            span,
            "std.floor() expected number, but got something else".to_string(),
            source_id,
        )),
    }
}

/// std.ceil(x): Returns the ceiling of x
fn std_ceil(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.ceil())),
        _ => Err(RuntimeError::new(
            span,
            "std.ceil() expected number, but got something else".to_string(),
            source_id,
        )),
    }
}

/// std.round(x): Returns x rounded to the nearest integer using floor(x + 0.5) per spec
fn std_round(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number((n + 0.5).floor())),
        _ => Err(RuntimeError::new(
            span,
            "std.round() expected number, but got something else".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.min() expected two numbers".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.max() expected two numbers".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.sign() expected number, but got something else".to_string(),
            source_id,
        )),
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
            // Walk the full base_object chain; shallower nodes win on collision
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let visible_keys: Vec<StringIndex> = fields
                .into_iter()
                .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
                .map(|(key, _, _)| key)
                .collect();
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
        _ => Err(RuntimeError::new(
            span,
            "std.objectFields() expected object, but got something else".to_string(),
            source_id,
        )),
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
            let field_name = memory_manager.load_string(s_idx).to_string();
            // Walk chain; only visible fields count for objectHas
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let found = fields.iter().any(|(key, _, vis)| {
                *vis != FieldVisibility::Hidden && memory_manager.load_string(*key) == field_name
            });
            Ok(Value::Boolean(found))
        }
        (Value::Object(_), _) => Err(RuntimeError::new(
            span,
            "std.objectHas() second argument must be a string".to_string(),
            source_id,
        )),
        _ => Err(RuntimeError::new(
            span,
            "std.objectHas() first argument must be an object".to_string(),
            source_id,
        )),
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
            // Walk chain; collect visible (key, value) pairs
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let visible_pairs: Vec<(StringIndex, Value)> = fields
                .into_iter()
                .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
                .map(|(key, val, _)| (key, val))
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
        _ => Err(RuntimeError::new(
            span,
            "std.objectValues() expected object, but got something else".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.range() expected two numbers".to_string(),
            source_id,
        )),
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
                Err(_) => Err(RuntimeError::new(
                    span,
                    format!("std.parseInt() failed to parse '{}' as integer", s),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.parseInt() expected string, but got something else".to_string(),
            source_id,
        )),
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
                Err(_) => Err(RuntimeError::new(
                    span,
                    format!("std.parseOctal() failed to parse '{}' as octal", s),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.parseOctal() expected string, but got something else".to_string(),
            source_id,
        )),
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
                Err(_) => Err(RuntimeError::new(
                    span,
                    format!("std.parseHex() failed to parse '{}' as hex", s),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.parseHex() expected string, but got something else".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.asciiUpper() expected string, but got something else".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.asciiLower() expected string, but got something else".to_string(),
            source_id,
        )),
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
            // Collect visible (key_string, value) pairs walking the full chain, sorted by key name
            let get_visible = |obj_idx, mm: &MemoryManager| -> Vec<(String, Value)> {
                let fields = mm.collect_object_fields_chain(obj_idx);
                let mut named: Vec<(String, Value)> = fields
                    .into_iter()
                    .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
                    .map(|(k, v, _)| (mm.load_string(k).to_string(), v))
                    .collect();
                named.sort_by(|a, b| a.0.cmp(&b.0));
                named
            };
            let ox = get_visible(x, mm);
            let oy = get_visible(y, mm);
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
            return Err(RuntimeError::new(
                span,
                "std.substr() expects (string, number, number)".to_string(),
                source_id,
            ));
        }
    };
    if from_n < 0.0 || from_n.fract() != 0.0 {
        return Err(RuntimeError::new(
            span,
            format!(
                "std.substr() 'from' must be a non-negative integer, got {}",
                from_n
            ),
            source_id,
        ));
    }
    if len_n < 0.0 || len_n.fract() != 0.0 {
        return Err(RuntimeError::new(
            span,
            format!(
                "std.substr() 'len' must be a non-negative integer, got {}",
                len_n
            ),
            source_id,
        ));
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
            return Err(RuntimeError::new(
                span,
                "std.split() expects (string, string)".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.join() second argument must be an array".to_string(),
                source_id,
            ));
        }
    };

    match sep_val {
        Value::String(sep_idx) => {
            // String mode: join array of strings with separator (nulls skipped)
            let sep = memory_manager.load_string(sep_idx).to_string();
            let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
            let mut parts: Vec<String> = Vec::with_capacity(elements.len());
            for elem in &elements {
                match elem {
                    Value::Null => continue,
                    Value::String(s_idx) => {
                        parts.push(memory_manager.load_string(*s_idx).to_string());
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "std.join() with string separator requires array of strings"
                                .to_string(),
                            source_id,
                        ));
                    }
                }
            }
            let result = parts.join(&sep);
            let alloc = memory_manager.allocate_string(&result);
            Ok(Value::String(alloc.index))
        }
        Value::Array(sep_arr_idx) => {
            // Array mode: interleave sep array between sub-arrays (nulls skipped)
            let sep_elements: Vec<Value> = memory_manager.load_array(sep_arr_idx).elements.clone();
            let outer_elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
            let mut result: Vec<Value> = Vec::new();
            let mut first = true;
            for elem in outer_elements.iter() {
                match elem {
                    Value::Null => continue,
                    Value::Array(sub_idx) => {
                        if !first {
                            result.extend(sep_elements.clone());
                        }
                        first = false;
                        let sub_elements: Vec<Value> =
                            memory_manager.load_array(*sub_idx).elements.clone();
                        result.extend(sub_elements);
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "std.join() with array separator requires array of arrays".to_string(),
                            source_id,
                        ));
                    }
                }
            }
            let arr_alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.join() first argument must be a string or array".to_string(),
            source_id,
        )),
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
            return Err(RuntimeError::new(
                span,
                "std.lines() expected array, but got something else".to_string(),
                source_id,
            ));
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut result = String::new();
    for elem in &elements {
        match elem {
            Value::Null => continue,
            Value::String(s_idx) => {
                result.push_str(memory_manager.load_string(*s_idx));
                result.push('\n');
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.lines() expected array of strings".to_string(),
                    source_id,
                ));
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
            return Err(RuntimeError::new(
                span,
                "std.stringChars() expected string, but got something else".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.flattenArrays() expected array, but got something else".to_string(),
                source_id,
            ));
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
                return Err(RuntimeError::new(
                    span,
                    "std.flattenArrays() expected array of arrays".to_string(),
                    source_id,
                ));
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
            return Err(RuntimeError::new(
                span,
                "std.reverse() expected array, but got something else".to_string(),
                source_id,
            ));
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
                    return Err(RuntimeError::new(
                        span,
                        "std.member() with string haystack requires string needle".to_string(),
                        source_id,
                    ));
                }
            };
            let haystack = memory_manager.load_string(s_idx).to_string();
            let needle = memory_manager.load_string(needle_idx).to_string();
            Ok(Value::Boolean(haystack.contains(needle.as_str())))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.member() first argument must be an array or string".to_string(),
            source_id,
        )),
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
            return Err(RuntimeError::new(
                span,
                "std.count() expected array as first argument".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.find() expected array as second argument".to_string(),
                source_id,
            ));
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
        _ => Err(RuntimeError::new(
            span,
            "std.clamp() expected three numbers".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.startsWith() expected two strings".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.endsWith() expected two strings".to_string(),
            source_id,
        )),
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
                // Empty pattern returns empty array (official jsonnet behavior)
                Vec::new()
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
                    }
                    i += 1;
                }
                result
            };

            let arr_alloc = memory_manager.allocate_array(indices);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.findSubstr() expected two strings".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.strReplace() expected three strings".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.isEmpty() expected string, but got something else".to_string(),
            source_id,
        )),
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
            return Err(RuntimeError::new(
                span,
                "std.all() expected array, but got something else".to_string(),
                source_id,
            ));
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
                return Err(RuntimeError::new(
                    span,
                    "std.all() expected array of booleans".to_string(),
                    source_id,
                ));
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
            return Err(RuntimeError::new(
                span,
                "std.any() expected array, but got something else".to_string(),
                source_id,
            ));
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
                return Err(RuntimeError::new(
                    span,
                    "std.any() expected array of booleans".to_string(),
                    source_id,
                ));
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
            return Err(RuntimeError::new(
                span,
                "std.sum() expected array, but got something else".to_string(),
                source_id,
            ));
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    let mut total = 0.0f64;
    for elem in &elements {
        match elem {
            Value::Number(n) => total += n,
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.sum() expected array of numbers".to_string(),
                    source_id,
                ));
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
        Err(RuntimeError::new(
            span,
            format!(
                "Assertion failed: {} was not equal to {}",
                a_display, b_display
            ),
            source_id,
        ))
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
                let fields = mm.collect_object_fields_chain(o_idx);
                let pairs: Vec<(String, Value)> = fields
                    .into_iter()
                    .map(|(k, v, _)| (mm.load_string(k).to_string(), v))
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
            FormatVals::Array(v) => v.get(idx).copied().ok_or_else(|| {
                RuntimeError::new(
                    span.clone(),
                    format!(
                        "std.format: index {} out of range (array has {} elements)",
                        idx,
                        v.len()
                    ),
                    source_id.to_string(),
                )
            }),
            FormatVals::Single(v) => {
                if idx == 0 {
                    Ok(*v)
                } else {
                    Err(RuntimeError::new(
                        span.clone(),
                        format!("std.format: index {} out of range (single value)", idx),
                        source_id.to_string(),
                    ))
                }
            }
            FormatVals::Object(_) => Err(RuntimeError::new(
                span.clone(),
                "std.format: positional index used with object values".to_string(),
                source_id.to_string(),
            )),
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
                Err(RuntimeError::new(
                    span.clone(),
                    format!("std.format: key '{}' not found in object", key),
                    source_id.to_string(),
                ))
            }
            _ => Err(RuntimeError::new(
                span.clone(),
                "std.format: named arg used but values is not an object".to_string(),
                source_id.to_string(),
            )),
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
        Value::Array(a_idx) => {
            let arr = mm.load_array(a_idx);
            if arr.elements.is_empty() {
                "[ ]".to_string()
            } else {
                let items: Vec<String> = arr
                    .elements
                    .iter()
                    .map(|v| value_to_string_repr(*v, mm))
                    .collect();
                format!("[{}]", items.join(", "))
            }
        }
        Value::Object(o_idx) => {
            let fields = mm.collect_object_fields_chain(o_idx);
            let visible: Vec<_> = fields
                .into_iter()
                .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
                .collect();
            if visible.is_empty() {
                "{ }".to_string()
            } else {
                let items: Vec<String> = visible
                    .iter()
                    .map(|(k, v, _)| {
                        let key = mm.load_string(*k);
                        let val = value_to_string_repr(*v, mm);
                        format!("{}: {}", key, val)
                    })
                    .collect();
                format!("{{ {} }}", items.join(", "))
            }
        }
        Value::Function(_) | Value::Closure(_) | Value::NativeFunction(_) => {
            "<function>".to_string()
        }
        _ => "<value>".to_string(),
    }
}

fn value_to_string_repr(val: Value, mm: &MemoryManager) -> String {
    match val {
        Value::String(idx) => format!("\"{}\"", mm.load_string(idx)),
        _ => value_to_format_string(val, mm),
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
            return Err(RuntimeError::new(
                span.clone(),
                "std.format: trailing '%' in format string".to_string(),
                source_id.to_string(),
            ));
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
                return Err(RuntimeError::new(
                    span.clone(),
                    "std.format: unclosed '(' in format specifier".to_string(),
                    source_id.to_string(),
                ));
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

        // Parse width (* means take from args)
        let mut width: usize = 0;
        if i < chars.len() && chars[i] == '*' {
            let w_val = vals.get_positional(pos_idx, span, source_id)?;
            pos_idx += 1;
            if let Value::Number(n) = w_val {
                let w = n as i64;
                if w < 0 {
                    flags.push('-');
                    width = (-w) as usize;
                } else {
                    width = w as usize;
                }
            }
            i += 1;
        } else {
            while i < chars.len() && chars[i].is_ascii_digit() {
                width = width * 10 + (chars[i] as usize - '0' as usize);
                i += 1;
            }
        }

        // Parse .precision (* means take from args)
        let mut precision: Option<usize> = None;
        if i < chars.len() && chars[i] == '.' {
            i += 1;
            if i < chars.len() && chars[i] == '*' {
                let p_val = vals.get_positional(pos_idx, span, source_id)?;
                pos_idx += 1;
                if let Value::Number(n) = p_val {
                    precision = Some(n as usize);
                }
                i += 1;
            } else {
                let mut p: usize = 0;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    p = p * 10 + (chars[i] as usize - '0' as usize);
                    i += 1;
                }
                precision = Some(p);
            }
        }

        if i >= chars.len() {
            return Err(RuntimeError::new(
                span.clone(),
                "std.format: incomplete format specifier".to_string(),
                source_id.to_string(),
            ));
        }

        // Skip C length modifiers (ignored in Jsonnet)
        while i < chars.len() && "hlL".contains(chars[i]) {
            i += 1;
        }
        if i >= chars.len() {
            return Err(RuntimeError::new(
                span.clone(),
                "std.format: incomplete format specifier".to_string(),
                source_id.to_string(),
            ));
        }

        let conv = chars[i];
        i += 1;

        // %% or %<flags>% → literal percent (no value consumed)
        if conv == '%' {
            let s = "%".to_string();
            if width > 0 {
                if flags.contains('-') {
                    result.push_str(&format!("{:<width$}", s, width = width));
                } else {
                    result.push_str(&format!("{:>width$}", s, width = width));
                }
            } else {
                result.push_str(&s);
            }
            continue;
        }

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
            'd' | 'i' | 'u' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            format!("std.format: %{} requires a number", conv),
                            source_id.to_string(),
                        ));
                    }
                };
                let int_val = n as i64;
                let abs_str = format!("{}", int_val.unsigned_abs());
                let s = if let Some(p) = precision {
                    if p > abs_str.len() {
                        let padded = format!("{:0>width$}", abs_str, width = p);
                        if int_val < 0 {
                            format!("-{}", padded)
                        } else {
                            padded
                        }
                    } else if p == 0 && int_val == 0 {
                        String::new()
                    } else {
                        format!("{}", int_val)
                    }
                } else {
                    format!("{}", int_val)
                };
                apply_numeric_format(&s, &flags, width, zero_pad && precision.is_none())
            }
            'o' | 'x' | 'X' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            format!("std.format: %{} requires a number", conv),
                            source_id.to_string(),
                        ));
                    }
                };
                let int_val = n as i64;
                let abs_digits = match conv {
                    'o' => format!("{:o}", int_val.unsigned_abs()),
                    'x' => format!("{:x}", int_val.unsigned_abs()),
                    'X' => format!("{:X}", int_val.unsigned_abs()),
                    _ => unreachable!(),
                };
                // Apply precision (minimum digits)
                let abs_digits = if let Some(p) = precision {
                    if p == 0 && int_val == 0 {
                        String::new()
                    } else if p > abs_digits.len() {
                        format!("{:0>width$}", abs_digits, width = p)
                    } else {
                        abs_digits
                    }
                } else {
                    abs_digits
                };
                // Build sign
                let sign = if int_val < 0 {
                    "-"
                } else if flags.contains('+') {
                    "+"
                } else if flags.contains(' ') {
                    " "
                } else {
                    ""
                };
                // Build prefix for # flag
                let prefix = if flags.contains('#') && int_val != 0 {
                    match conv {
                        'o' => {
                            if !abs_digits.starts_with('0') {
                                "0"
                            } else {
                                ""
                            }
                        }
                        'x' => "0x",
                        'X' => "0X",
                        _ => "",
                    }
                } else {
                    ""
                };
                let content_len = sign.len() + prefix.len() + abs_digits.len();
                if width == 0 || content_len >= width {
                    format!("{}{}{}", sign, prefix, abs_digits)
                } else {
                    let padding = width - content_len;
                    if flags.contains('-') {
                        format!("{}{}{}{}", sign, prefix, abs_digits, " ".repeat(padding))
                    } else if zero_pad && precision.is_none() {
                        format!("{}{}{}{}", sign, prefix, "0".repeat(padding), abs_digits)
                    } else {
                        format!("{}{}{}{}", " ".repeat(padding), sign, prefix, abs_digits)
                    }
                }
            }
            'f' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            "std.format: %f requires a number".to_string(),
                            source_id.to_string(),
                        ));
                    }
                };
                let p = precision.unwrap_or(6);
                let mut s = format!("{:.prec$}", n, prec = p);
                if flags.contains('#') && !s.contains('.') {
                    s.push('.');
                }
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'e' | 'E' => {
                let upper = conv == 'E';
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            format!("std.format: %{} requires a number", conv),
                            source_id.to_string(),
                        ));
                    }
                };
                let p = precision.unwrap_or(6);
                let s = format!("{:.prec$e}", n, prec = p);
                let mut s = normalize_exp_notation(&s, upper);
                // # flag: ensure decimal point is present
                if flags.contains('#') && !s.contains('.') {
                    let e_char = if upper { 'E' } else { 'e' };
                    if let Some(pos) = s.find(e_char) {
                        s.insert(pos, '.');
                    }
                }
                apply_numeric_format(&s, &flags, width, zero_pad)
            }
            'g' | 'G' => {
                let n = match val {
                    Value::Number(n) => n,
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            format!("std.format: %{} requires a number", conv),
                            source_id.to_string(),
                        ));
                    }
                };
                let upper = conv == 'G';
                let p = precision.unwrap_or(6).max(1);
                let s = if flags.contains('#') {
                    format_g_alt(n, p, upper)
                } else {
                    format_g(n, p, upper)
                };
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
                                return Err(RuntimeError::new(
                                    span.clone(),
                                    "std.format: %c requires non-empty string or number"
                                        .to_string(),
                                    source_id.to_string(),
                                ));
                            }
                        }
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            span.clone(),
                            "std.format: %c requires a number or string".to_string(),
                            source_id.to_string(),
                        ));
                    }
                };
                let c = char::from_u32(n).unwrap_or('\u{FFFD}');
                apply_width_align(&c.to_string(), &flags, width)
            }
            _ => {
                return Err(RuntimeError::new(
                    span.clone(),
                    format!("std.format: unknown format specifier '%{}'", conv),
                    source_id.to_string(),
                ));
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

/// Format a float using %#g/%#G semantics (keep trailing zeros, always show decimal point)
fn format_g_alt(n: f64, prec: usize, upper: bool) -> String {
    if n == 0.0 {
        let p = if prec > 1 { prec - 1 } else { 0 };
        let mut s = format!("{:.prec$}", 0.0, prec = p);
        if !s.contains('.') {
            s.push('.');
        }
        return s;
    }
    // Format using %e first to determine the exponent after rounding
    let p = if prec > 0 { prec - 1 } else { 0 };
    let e_str = format!("{:.prec$e}", n, prec = p);
    let exp: i32 = e_str
        .split('e')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if exp < -4 || exp >= prec as i32 {
        let s = normalize_exp_notation(&e_str, upper);
        // Ensure decimal point
        let e_char = if upper { 'E' } else { 'e' };
        if let Some(epos) = s.find(e_char) {
            if !s[..epos].contains('.') {
                let mut result = s[..epos].to_string();
                result.push('.');
                result.push_str(&s[epos..]);
                return result;
            }
        }
        s
    } else {
        let decimal_places = if prec as i32 > exp + 1 {
            (prec as i32 - exp - 1) as usize
        } else {
            0
        };
        let mut s = format!("{:.prec$}", n, prec = decimal_places);
        if !s.contains('.') {
            s.push('.');
        }
        s
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
            return Err(RuntimeError::new(
                span,
                "std.format() first argument must be a string".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.splitLimit() expects (string, string, number)".to_string(),
                source_id,
            ));
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
                return Err(RuntimeError::new(
                    span,
                    format!(
                        "std.repeat() count must be a non-negative integer, got {}",
                        n
                    ),
                    source_id,
                ));
            }
            n as usize
        }
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.repeat() count must be a number".to_string(),
                source_id,
            ));
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
        _ => Err(RuntimeError::new(
            span,
            "std.repeat() first argument must be a string or array".to_string(),
            source_id,
        )),
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
        _ => Err(RuntimeError::new(
            span,
            "std.slice() first argument must be a string or array".to_string(),
            source_id,
        )),
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
                return Err(RuntimeError::new(
                    span.clone(),
                    format!("std.slice() step must be a positive integer, got {}", n),
                    source_id.to_string(),
                ));
            }
            n as usize
        }
        _ => {
            return Err(RuntimeError::new(
                span.clone(),
                "std.slice() step must be a number or null".to_string(),
                source_id.to_string(),
            ));
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
            return Err(RuntimeError::new(
                span.clone(),
                "std.slice() index must be a number or null".to_string(),
                source_id.to_string(),
            ));
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
            return Err(RuntimeError::new(
                span.clone(),
                "std.slice() end must be a number or null".to_string(),
                source_id.to_string(),
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.get() first argument must be an object".to_string(),
                source_id,
            ));
        }
    };

    let field_name = match f_val {
        Value::String(s_idx) => memory_manager.load_string(s_idx).to_string(),
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.get() second argument must be a string".to_string(),
                source_id,
            ));
        }
    };

    let inc_hidden = match inc_hidden_val {
        Value::Boolean(b) => b,
        Value::Null => true,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.get() fourth argument must be a boolean".to_string(),
                source_id,
            ));
        }
    };

    // Walk the full chain to find the field
    let fields = memory_manager.collect_object_fields_chain(o_idx);
    let found: Option<(Value, chunk::FieldVisibility)> = fields
        .into_iter()
        .find(|(k, _, _)| memory_manager.load_string(*k) == field_name.as_str())
        .map(|(_, v, vis)| (v, vis));

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
            // Walk chain; all fields (including hidden) count for objectHasAll
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let found = fields
                .iter()
                .any(|(key, _, _)| memory_manager.load_string(*key) == field_name);
            Ok(Value::Boolean(found))
        }
        (Value::Object(_), _) => Err(RuntimeError::new(
            span,
            "std.objectHasAll() second argument must be a string".to_string(),
            source_id,
        )),
        _ => Err(RuntimeError::new(
            span,
            "std.objectHasAll() first argument must be an object".to_string(),
            source_id,
        )),
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
            // Walk the full base_object chain; shallower nodes win on collision
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let all_keys: Vec<StringIndex> = fields.into_iter().map(|(key, _, _)| key).collect();
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
        _ => Err(RuntimeError::new(
            span,
            "std.objectFieldsAll() expected object, but got something else".to_string(),
            source_id,
        )),
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
            return Err(RuntimeError::new(
                span,
                "std.encodeUTF8() expected string, but got something else".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.decodeUTF8() expected array, but got something else".to_string(),
                source_id,
            ));
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(a_idx).elements.clone();
    let mut bytes: Vec<u8> = Vec::with_capacity(elements.len());
    for elem in &elements {
        match elem {
            Value::Number(n) => {
                if *n < 0.0 || *n > 255.0 || n.fract() != 0.0 {
                    return Err(RuntimeError::new(
                        span,
                        format!("std.decodeUTF8() byte value out of range: {}", n),
                        source_id,
                    ));
                }
                bytes.push(*n as u8);
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.decodeUTF8() array must contain numbers".to_string(),
                    source_id,
                ));
            }
        }
    }
    match std::str::from_utf8(&bytes) {
        Ok(s) => {
            let alloc = memory_manager.allocate_string(s);
            Ok(Value::String(alloc.index))
        }
        Err(e) => Err(RuntimeError::new(
            span,
            format!("std.decodeUTF8() invalid UTF-8 sequence: {}", e),
            source_id,
        )),
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
pub fn std_sort(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.sort() expected array, but got something else".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.splitLimitR() expects (string, string, number)".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.stripChars() expects (string, string)".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.lstripChars() expects (string, string)".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.rstripChars() expects (string, string)".to_string(),
                source_id,
            ));
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
            return Err(RuntimeError::new(
                span,
                "std.trim() expected string, but got something else".to_string(),
                source_id,
            ));
        }
    };
    let s = memory_manager.load_string(s_idx).to_string();
    let whitespace: HashSet<char> = [
        ' ', '\t', '\n', '\r', '\x0B', '\x0C', '\u{0085}', '\u{00A0}',
    ]
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
            return Err(RuntimeError::new(
                span,
                "std.objectKeysValues() expected object, but got something else".to_string(),
                source_id,
            ));
        }
    };
    // Collect visible (key_name, value) pairs walking the full chain
    let fields = memory_manager.collect_object_fields_chain(o_idx);
    let visible_pairs: Vec<(StringIndex, Value)> = fields
        .into_iter()
        .filter(|(_, _, vis)| *vis != FieldVisibility::Hidden)
        .map(|(key, val, _)| (key, val))
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
            ObjectField::new(Value::String(name_str_idx), FieldVisibility::Visible),
        );
        properties.insert(
            value_field_name,
            ObjectField::new(val, FieldVisibility::Visible),
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
            return Err(RuntimeError::new(
                span,
                "std.avg() expected array, but got something else".to_string(),
                source_id,
            ));
        }
    };
    let elements: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();
    if elements.is_empty() {
        return Err(RuntimeError::new(
            span,
            "std.avg() array must be non-empty".to_string(),
            source_id,
        ));
    }
    let mut total = 0.0f64;
    for elem in &elements {
        match elem {
            Value::Number(n) => total += n,
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.avg() expected array of numbers".to_string(),
                    source_id,
                ));
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
            return Err(RuntimeError::new(
                span,
                "std.remove() first argument must be an array".to_string(),
                source_id,
            ));
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
pub fn display_value(val: Value, memory_manager: &MemoryManager) -> String {
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

/// Pure Rust base64 encoder
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let combined = (b0 << 16) | (b1 << 8) | b2;
        result.push(ALPHABET[((combined >> 18) & 0x3F) as usize] as char);
        result.push(ALPHABET[((combined >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(ALPHABET[((combined >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[(combined & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

/// Pure Rust base64 decoder
fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let s = s.trim_end_matches('=');
    let mut bits = 0u32;
    let mut bit_count = 0u32;
    let mut out = Vec::new();
    for ch in s.chars() {
        let val = ALPHABET
            .iter()
            .position(|&c| c == ch as u8)
            .ok_or_else(|| format!("invalid base64 character: {}", ch))? as u32;
        bits = (bits << 6) | val;
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            out.push(((bits >> bit_count) & 0xFF) as u8);
        }
    }
    Ok(out)
}

/// std.base64(input): encode string or array of bytes to base64 string
fn std_base64(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let bytes: Vec<u8> = match val {
        Value::String(s_idx) => memory_manager.load_string(s_idx).as_bytes().to_vec(),
        Value::Array(a_idx) => {
            let elements = memory_manager.load_array(a_idx).elements.clone();
            let mut bytes = Vec::with_capacity(elements.len());
            for elem in &elements {
                match elem {
                    Value::Number(n) => {
                        let n = *n;
                        if n < 0.0 || n > 255.0 || n.fract() != 0.0 {
                            return Err(RuntimeError::new(
                                span,
                                format!(
                                    "std.base64: array element must be a byte (0-255), got {}",
                                    n
                                ),
                                source_id,
                            ));
                        }
                        bytes.push(n as u8);
                    }
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "std.base64: array elements must be numbers".to_string(),
                            source_id,
                        ));
                    }
                }
            }
            bytes
        }
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.base64: argument must be a string or array of bytes".to_string(),
                source_id,
            ));
        }
    };
    let encoded = base64_encode(&bytes);
    let alloc = memory_manager.allocate_string(&encoded);
    Ok(Value::String(alloc.index))
}

/// std.base64DecodeBytes(str): decode base64 string to array of numbers 0-255
fn std_base64_decode_bytes(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            match base64_decode(&s) {
                Ok(bytes) => {
                    let elements: Vec<Value> =
                        bytes.iter().map(|&b| Value::Number(b as f64)).collect();
                    let arr_alloc = memory_manager.allocate_array(elements);
                    Ok(Value::Array(arr_alloc.index))
                }
                Err(e) => Err(RuntimeError::new(
                    span,
                    format!("std.base64DecodeBytes: {}", e),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.base64DecodeBytes: argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.escapeStringJson(str): escape string for JSON embedding with surrounding quotes
fn std_escape_string_json(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let mut out = String::from("\"");
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        out.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
            let alloc = memory_manager.allocate_string(&out);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.escapeStringJson: argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.escapeStringXml(str): escape <>&"' for XML/HTML
fn std_escape_string_xml(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let mut out = String::new();
            for ch in s.chars() {
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    '"' => out.push_str("&quot;"),
                    '\'' => out.push_str("&apos;"),
                    c => out.push(c),
                }
            }
            let alloc = memory_manager.allocate_string(&out);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.escapeStringXml: argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.escapeStringBash(str): wrap in single quotes, escape internal ' as '"'"'
fn std_escape_string_bash(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let mut out = String::from("'");
            for ch in s.chars() {
                if ch == '\'' {
                    out.push_str("'\"'\"'");
                } else {
                    out.push(ch);
                }
            }
            out.push('\'');
            let alloc = memory_manager.allocate_string(&out);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.escapeStringBash: argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.parseFloat(str): parse string to float
fn std_parse_float(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            match s.parse::<f64>() {
                Ok(n) => Ok(Value::Number(n)),
                Err(_) => Err(RuntimeError::new(
                    span,
                    format!("std.parseFloat: could not parse {:?}", s),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.parseFloat: argument must be a string".to_string(),
            source_id,
        )),
    }
}

// ─── std.pow ──────────────────────────────────────────────────────────────────

/// std.pow(x, n): Returns x raised to the power n
fn std_pow(
    x: Value,
    n: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (x, n) {
        (Value::Number(xv), Value::Number(nv)) => Ok(Value::Number(xv.powf(nv))),
        _ => Err(RuntimeError::new(
            span,
            "std.pow() expected two numbers".to_string(),
            source_id,
        )),
    }
}

// ─── std.sqrt ─────────────────────────────────────────────────────────────────

/// std.sqrt(x): Returns the square root of x
fn std_sqrt(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.sqrt())),
        _ => Err(RuntimeError::new(
            span,
            "std.sqrt() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.exp ──────────────────────────────────────────────────────────────────

/// std.exp(x): Returns e raised to the power x
fn std_exp(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.exp())),
        _ => Err(RuntimeError::new(
            span,
            "std.exp() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.log ──────────────────────────────────────────────────────────────────

/// std.log(x): Returns the natural logarithm of x
fn std_log(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.ln())),
        _ => Err(RuntimeError::new(
            span,
            "std.log() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.isEven ───────────────────────────────────────────────────────────────

/// std.isEven(x): Returns true if the integral part of x is even
fn std_is_even(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Boolean(n.trunc() % 2.0 == 0.0)),
        _ => Err(RuntimeError::new(
            span,
            "std.isEven() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.isOdd ────────────────────────────────────────────────────────────────

/// std.isOdd(x): Returns true if the integral part of x is odd
fn std_is_odd(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Boolean(n.trunc() % 2.0 != 0.0)),
        _ => Err(RuntimeError::new(
            span,
            "std.isOdd() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.contains ─────────────────────────────────────────────────────────────

/// std.contains(arr, elem): Returns true if arr contains elem
fn std_contains(
    arr_val: Value,
    elem: Value,
    memory_manager: &MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.contains() first argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let elements = memory_manager.load_array(arr_idx).elements.clone();
    let found = elements
        .iter()
        .any(|v| values_equal(*v, elem, memory_manager));
    Ok(Value::Boolean(found))
}

// ─── std.objectValuesAll ──────────────────────────────────────────────────────

/// std.objectValuesAll(o): Returns an array of all field values (including hidden), sorted by key
fn std_object_values_all(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Object(o_idx) => {
            // Walk chain; collect all (key, value) pairs including hidden
            let fields = memory_manager.collect_object_fields_chain(o_idx);
            let mut named_pairs: Vec<(String, Value)> = fields
                .into_iter()
                .map(|(key, val, _)| (memory_manager.load_string(key).to_string(), val))
                .collect();
            named_pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let elements: Vec<Value> = named_pairs.into_iter().map(|(_, v)| v).collect();
            let arr_alloc = memory_manager.allocate_array(elements);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.objectValuesAll() expected object, but got something else".to_string(),
            source_id,
        )),
    }
}

// ─── std.sin ──────────────────────────────────────────────────────────────────

/// std.sin(x): Returns the sine of x (x in radians)
fn std_sin(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.sin())),
        _ => Err(RuntimeError::new(
            span,
            "std.sin() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.cos ──────────────────────────────────────────────────────────────────

/// std.cos(x): Returns the cosine of x (x in radians)
fn std_cos(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.cos())),
        _ => Err(RuntimeError::new(
            span,
            "std.cos() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.tan ──────────────────────────────────────────────────────────────────

/// std.tan(x): Returns the tangent of x (x in radians)
fn std_tan(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.tan())),
        _ => Err(RuntimeError::new(
            span,
            "std.tan() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.log2 ─────────────────────────────────────────────────────────────────

/// std.log2(x): Returns the base-2 logarithm of x
fn std_log2(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.log2())),
        _ => Err(RuntimeError::new(
            span,
            "std.log2() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.log10 ────────────────────────────────────────────────────────────────

/// std.log10(x): Returns the base-10 logarithm of x
fn std_log10(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.log10())),
        _ => Err(RuntimeError::new(
            span,
            "std.log10() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.xor ──────────────────────────────────────────────────────────────────

/// std.xor(a, b): Returns the boolean XOR of a and b
fn std_xor(
    a: Value,
    b: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Boolean(x), Value::Boolean(y)) => Ok(Value::Boolean(x ^ y)),
        _ => Err(RuntimeError::new(
            span,
            "std.xor() expected two booleans".to_string(),
            source_id,
        )),
    }
}

// ─── std.xnor ─────────────────────────────────────────────────────────────────

/// std.xnor(a, b): Returns the boolean XNOR of a and b
fn std_xnor(
    a: Value,
    b: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Boolean(x), Value::Boolean(y)) => Ok(Value::Boolean(!(x ^ y))),
        _ => Err(RuntimeError::new(
            span,
            "std.xnor() expected two booleans".to_string(),
            source_id,
        )),
    }
}

// ─── std.objectKeysValuesAll ──────────────────────────────────────────────────

/// std.objectKeysValuesAll(o): Returns [{key, value}] for all fields (including hidden), sorted by key
fn std_object_keys_values_all(
    obj_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let o_idx = match obj_val {
        Value::Object(o) => o,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.objectKeysValuesAll() expected object, but got something else".to_string(),
                source_id,
            ));
        }
    };
    // Collect all (key_name, value) pairs walking the full chain — no visibility filter
    let fields = memory_manager.collect_object_fields_chain(o_idx);
    let mut named_pairs: Vec<(String, Value)> = fields
        .into_iter()
        .map(|(key, val, _)| (memory_manager.load_string(key).to_string(), val))
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
            ObjectField::new(Value::String(name_str_idx), FieldVisibility::Visible),
        );
        properties.insert(
            value_field_name,
            ObjectField::new(val, FieldVisibility::Visible),
        );
        let obj_alloc = memory_manager.allocate_object_with_properties(properties);
        result_elements.push(Value::Object(obj_alloc.index));
    }
    let arr_alloc = memory_manager.allocate_array(result_elements);
    Ok(Value::Array(arr_alloc.index))
}

// ─── std.asin ─────────────────────────────────────────────────────────────────

/// std.asin(x): Returns the arcsine of x in radians
fn std_asin(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.asin())),
        _ => Err(RuntimeError::new(
            span,
            "std.asin() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.acos ─────────────────────────────────────────────────────────────────

/// std.acos(x): Returns the arccosine of x in radians
fn std_acos(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.acos())),
        _ => Err(RuntimeError::new(
            span,
            "std.acos() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.atan ─────────────────────────────────────────────────────────────────

/// std.atan(x): Returns the arctangent of x in radians
fn std_atan(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n.atan())),
        _ => Err(RuntimeError::new(
            span,
            "std.atan() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.atan2 ────────────────────────────────────────────────────────────────

/// std.atan2(y, x): Returns the two-argument arctangent of y/x in radians
fn std_atan2(
    y: Value,
    x: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (y, x) {
        (Value::Number(yv), Value::Number(xv)) => Ok(Value::Number(yv.atan2(xv))),
        _ => Err(RuntimeError::new(
            span,
            "std.atan2() expected two numbers".to_string(),
            source_id,
        )),
    }
}

// ─── std.isInteger ────────────────────────────────────────────────────────────

/// std.isInteger(x): Returns true if x has no fractional part
fn std_is_integer(
    val: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Boolean(n.fract() == 0.0)),
        _ => Err(RuntimeError::new(
            span,
            "std.isInteger() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.isDecimal ────────────────────────────────────────────────────────────

/// std.isDecimal(x): Returns true if x has a fractional part
fn std_is_decimal(
    val: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Boolean(n.fract() != 0.0)),
        _ => Err(RuntimeError::new(
            span,
            "std.isDecimal() expected number".to_string(),
            source_id,
        )),
    }
}

// ─── std.objectRemoveKey ──────────────────────────────────────────────────────

/// std.objectRemoveKey(obj, key): Returns a new object with the named key removed
fn std_object_remove_key(
    obj_val: Value,
    key_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let o_idx = match obj_val {
        Value::Object(o) => o,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.objectRemoveKey() expected object as first argument".to_string(),
                source_id,
            ));
        }
    };
    let target_key = match key_val {
        Value::String(s_idx) => memory_manager.load_string(s_idx).to_string(),
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.objectRemoveKey() expected string as second argument".to_string(),
                source_id,
            ));
        }
    };
    // Walk the full chain and flatten, then filter out the target key
    let all_fields = memory_manager.collect_object_fields_chain(o_idx);
    let mut new_properties = std::collections::HashMap::new();
    for (key_idx, val, vis) in all_fields {
        let key_name = memory_manager.load_string(key_idx).to_string();
        if key_name != target_key {
            new_properties.insert(key_idx, ObjectField::new(val, vis));
        }
    }
    let obj_alloc = memory_manager.allocate_object_with_properties(new_properties);
    Ok(Value::Object(obj_alloc.index))
}

// ─── std.flattenDeepArray ─────────────────────────────────────────────────────

/// Recursive helper: pushes all non-array values into `out`, recursing into arrays
fn flatten_deep(val: Value, out: &mut Vec<Value>, mm: &MemoryManager) {
    match val {
        Value::Array(a_idx) => {
            let elements = mm.load_array(a_idx).elements.clone();
            for elem in elements {
                flatten_deep(elem, out, mm);
            }
        }
        other => out.push(other),
    }
}

/// std.flattenDeepArray(arr): Recursively flattens all nested arrays into a single flat array
fn std_flatten_deep_array(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match arr_val {
        Value::Array(_) => {
            // Collect with immutable borrow first, then allocate
            let mut result = Vec::new();
            flatten_deep(arr_val, &mut result, memory_manager);
            let arr_alloc = memory_manager.allocate_array(result);
            Ok(Value::Array(arr_alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.flattenDeepArray() expected array".to_string(),
            source_id,
        )),
    }
}

fn std_deg2rad(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n * std::f64::consts::PI / 180.0)),
        _ => Err(RuntimeError::new(
            span,
            "std.deg2rad() expected number".to_string(),
            source_id,
        )),
    }
}

fn std_rad2deg(val: Value, span: Range<usize>, source_id: String) -> Result<Value, RuntimeError> {
    match val {
        Value::Number(n) => Ok(Value::Number(n * 180.0 / std::f64::consts::PI)),
        _ => Err(RuntimeError::new(
            span,
            "std.rad2deg() expected number".to_string(),
            source_id,
        )),
    }
}

fn std_hypot(
    a: Value,
    b: Value,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::Number(av), Value::Number(bv)) => Ok(Value::Number(av.hypot(bv))),
        _ => Err(RuntimeError::new(
            span,
            "std.hypot() expected two numbers".to_string(),
            source_id,
        )),
    }
}

fn std_remove_at(
    arr_val: Value,
    idx_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(a) => a,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.removeAt() first argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let idx = match idx_val {
        Value::Number(n) if n.fract() == 0.0 => n as i64,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.removeAt() second argument must be an integer".to_string(),
                source_id,
            ));
        }
    };
    let mut elements = memory_manager.load_array(arr_idx).elements.clone();
    let len = elements.len() as i64;
    if idx < 0 || idx >= len {
        return Err(RuntimeError::new(
            span,
            format!(
                "std.removeAt() index {} out of bounds for array of length {}",
                idx, len
            ),
            source_id,
        ));
    }
    elements.remove(idx as usize);
    let arr_alloc = memory_manager.allocate_array(elements);
    Ok(Value::Array(arr_alloc.index))
}

fn std_escape_string_dollars(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            let out = s.replace('$', "$$");
            let alloc = memory_manager.allocate_string(&out);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.escapeStringDollars() expected string".to_string(),
            source_id,
        )),
    }
}

fn std_equals_ignore_case(
    a: Value,
    b: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match (a, b) {
        (Value::String(a_idx), Value::String(b_idx)) => {
            let sa = memory_manager.load_string(a_idx).to_string();
            let sb = memory_manager.load_string(b_idx).to_string();
            Ok(Value::Boolean(sa.eq_ignore_ascii_case(&sb)))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.equalsIgnoreCase() expected two strings".to_string(),
            source_id,
        )),
    }
}

fn std_trace(
    str_val: Value,
    rest_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match str_val {
        Value::String(s_idx) => {
            let msg = memory_manager.load_string(s_idx).to_string();
            eprintln!("TRACE: {} {}", source_id, msg);
            Ok(rest_val)
        }
        _ => Err(RuntimeError::new(
            span,
            "std.trace() first argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.base64Decode(str): decode base64 to naive string (byte→char cast)
fn std_base64_decode_string(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(s_idx) => {
            let s = memory_manager.load_string(s_idx).to_string();
            match base64_decode(&s) {
                Ok(bytes) => {
                    let decoded: String = bytes.iter().map(|&b| b as char).collect();
                    let alloc = memory_manager.allocate_string(&decoded);
                    Ok(Value::String(alloc.index))
                }
                Err(e) => Err(RuntimeError::new(
                    span,
                    format!("std.base64Decode: {}", e),
                    source_id,
                )),
            }
        }
        _ => Err(RuntimeError::new(
            span,
            "std.base64Decode: argument must be a string".to_string(),
            source_id,
        )),
    }
}

/// std.minArray(arr): return the minimum element
fn std_min_array(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Array(a_idx) => {
            let elements = memory_manager.load_array(a_idx).elements.clone();
            if elements.is_empty() {
                return Err(RuntimeError::new(
                    span,
                    "std.minArray: array must not be empty".to_string(),
                    source_id,
                ));
            }
            let mut min = elements[0];
            for elem in elements.into_iter().skip(1) {
                if compare_values(elem, min, memory_manager) == std::cmp::Ordering::Less {
                    min = elem;
                }
            }
            Ok(min)
        }
        _ => Err(RuntimeError::new(
            span,
            "std.minArray: argument must be an array".to_string(),
            source_id,
        )),
    }
}

/// std.maxArray(arr): return the maximum element
fn std_max_array(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::Array(a_idx) => {
            let elements = memory_manager.load_array(a_idx).elements.clone();
            if elements.is_empty() {
                return Err(RuntimeError::new(
                    span,
                    "std.maxArray: array must not be empty".to_string(),
                    source_id,
                ));
            }
            let mut max = elements[0];
            for elem in elements.into_iter().skip(1) {
                if compare_values(elem, max, memory_manager) == std::cmp::Ordering::Greater {
                    max = elem;
                }
            }
            Ok(max)
        }
        _ => Err(RuntimeError::new(
            span,
            "std.maxArray: argument must be an array".to_string(),
            source_id,
        )),
    }
}

/// Helper: recursively collect strings from nested arrays into a buffer
fn deep_join_append(val: Value, buf: &mut String, memory_manager: &MemoryManager) {
    match val {
        Value::String(s_idx) => {
            buf.push_str(memory_manager.load_string(s_idx));
        }
        Value::Array(a_idx) => {
            let elements = memory_manager.load_array(a_idx).elements.clone();
            for elem in elements {
                deep_join_append(elem, buf, memory_manager);
            }
        }
        _ => {} // ignore other types
    }
}

/// std.deepJoin(arr): recursively concatenate strings in nested arrays
fn std_deep_join(
    val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    match val {
        Value::String(_) | Value::Array(_) => {
            let mut buf = String::new();
            deep_join_append(val, &mut buf, memory_manager);
            let alloc = memory_manager.allocate_string(&buf);
            Ok(Value::String(alloc.index))
        }
        _ => Err(RuntimeError::new(
            span,
            "std.deepJoin: argument must be a string or array".to_string(),
            source_id,
        )),
    }
}

/// Decompose a float into (mantissa, exponent) such that x = mantissa * 2^exponent,
/// with 0.5 <= |mantissa| < 1.0 (matching C's frexp semantics).
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return (x, 0);
    }
    let bits = x.to_bits();
    let exp_bits = ((bits >> 52) & 0x7ff) as i32;
    if exp_bits == 0 {
        // Subnormal: normalize first
        let norm = x * (1u64 << 52) as f64;
        let norm_bits = norm.to_bits();
        let norm_exp = ((norm_bits >> 52) & 0x7ff) as i32 - 1022 - 52;
        let mantissa = f64::from_bits((norm_bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
        let sign = if x < 0.0 { -1.0f64 } else { 1.0f64 };
        return (mantissa * sign, norm_exp);
    }
    let exp = exp_bits - 1022;
    let mantissa = f64::from_bits((bits & !(0x7ffu64 << 52)) | (1022u64 << 52));
    (mantissa, exp)
}

// ─── std.chunk ────────────────────────────────────────────────────────────────

/// std.chunk(arr, size): Split arr into consecutive sub-arrays of length size
/// (the last chunk may be shorter if the array length is not a multiple of size).
fn std_chunk(
    arr_val: Value,
    size_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.chunk: first argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let size = match size_val {
        Value::Number(n) if n >= 1.0 && n.fract() == 0.0 => n as usize,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.chunk: second argument must be a positive integer".to_string(),
                source_id,
            ));
        }
    };
    let elements = memory_manager.load_array(arr_idx).elements.clone();
    let mut chunks: Vec<Value> = Vec::new();
    for window in elements.chunks(size) {
        let sub = memory_manager.allocate_array(window.to_vec());
        chunks.push(Value::Array(sub.index));
    }
    let alloc = memory_manager.allocate_array(chunks);
    Ok(Value::Array(alloc.index))
}

// ─── std.zip ──────────────────────────────────────────────────────────────────

/// std.zip(arr1, arr2): Pair up elements from two arrays into an array of 2-element arrays,
/// truncating to the length of the shorter array.
fn std_zip(
    arr1_val: Value,
    arr2_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let a_idx = match arr1_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.zip: first argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let b_idx = match arr2_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.zip: second argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let a_elems = memory_manager.load_array(a_idx).elements.clone();
    let b_elems = memory_manager.load_array(b_idx).elements.clone();
    let len = a_elems.len().min(b_elems.len());
    let mut result: Vec<Value> = Vec::with_capacity(len);
    for i in 0..len {
        let pair = memory_manager.allocate_array(vec![a_elems[i], b_elems[i]]);
        result.push(Value::Array(pair.index));
    }
    let alloc = memory_manager.allocate_array(result);
    Ok(Value::Array(alloc.index))
}

// ─── std.unzip ────────────────────────────────────────────────────────────────

/// std.unzip(arr): Convert an array of 2-element arrays into a pair of arrays
/// [firsts, seconds].
fn std_unzip(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.unzip: argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let elements = memory_manager.load_array(arr_idx).elements.clone();
    let mut firsts: Vec<Value> = Vec::with_capacity(elements.len());
    let mut seconds: Vec<Value> = Vec::with_capacity(elements.len());
    for &elem in &elements {
        match elem {
            Value::Array(pair_idx) => {
                let pair = memory_manager.load_array(pair_idx).elements.clone();
                if pair.len() != 2 {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "std.unzip: each element must be a 2-element array, got length {}",
                            pair.len()
                        ),
                        source_id,
                    ));
                }
                firsts.push(pair[0]);
                seconds.push(pair[1]);
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.unzip: each element must be an array".to_string(),
                    source_id,
                ));
            }
        }
    }
    let a1 = memory_manager.allocate_array(firsts);
    let a2 = memory_manager.allocate_array(seconds);
    let result =
        memory_manager.allocate_array(vec![Value::Array(a1.index), Value::Array(a2.index)]);
    Ok(Value::Array(result.index))
}

// ─── std.objectFromPairs ──────────────────────────────────────────────────────

/// std.objectFromPairs(arr): Convert an array of [key, value] pairs into an object.
fn std_object_from_pairs(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.objectFromPairs: argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    let elements = memory_manager.load_array(arr_idx).elements.clone();
    // Collect (key_string, value) pairs — two-phase borrow
    let mut pairs: Vec<(String, Value)> = Vec::with_capacity(elements.len());
    for &elem in &elements {
        match elem {
            Value::Array(pair_idx) => {
                let pair = memory_manager.load_array(pair_idx).elements.clone();
                if pair.len() != 2 {
                    return Err(RuntimeError::new(
                        span,
                        format!(
                            "std.objectFromPairs: each element must be a 2-element array, got length {}",
                            pair.len()
                        ),
                        source_id,
                    ));
                }
                let key = match pair[0] {
                    Value::String(si) => memory_manager.load_string(si).to_string(),
                    _ => {
                        return Err(RuntimeError::new(
                            span,
                            "std.objectFromPairs: keys must be strings".to_string(),
                            source_id,
                        ));
                    }
                };
                pairs.push((key, pair[1]));
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.objectFromPairs: elements must be 2-element arrays".to_string(),
                    source_id,
                ));
            }
        }
    }
    let mut properties = std::collections::HashMap::new();
    for (key, val) in pairs {
        let key_idx = memory_manager.allocate_string(&key).index;
        properties.insert(key_idx, ObjectField::new(val, FieldVisibility::Visible));
    }
    let obj_alloc = memory_manager.allocate_object_with_properties(properties);
    Ok(Value::Object(obj_alloc.index))
}

// ─── std.pick ─────────────────────────────────────────────────────────────────

/// std.pick(obj, keys): Return a new object containing only the fields whose names
/// appear in the array keys (missing keys are silently ignored).
fn std_pick(
    obj_val: Value,
    keys_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let obj_idx = match obj_val {
        Value::Object(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.pick: first argument must be an object".to_string(),
                source_id,
            ));
        }
    };
    let keys_idx = match keys_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.pick: second argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    // Collect desired key names
    let key_elems = memory_manager.load_array(keys_idx).elements.clone();
    let mut desired: HashSet<String> = HashSet::new();
    for &k in &key_elems {
        match k {
            Value::String(si) => {
                desired.insert(memory_manager.load_string(si).to_string());
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.pick: keys must be strings".to_string(),
                    source_id,
                ));
            }
        }
    }
    // Collect visible fields walking the full chain
    let all_fields = memory_manager.collect_object_fields_chain(obj_idx);
    let mut new_properties = std::collections::HashMap::new();
    for (key_idx, val, vis) in all_fields {
        if vis != FieldVisibility::Hidden {
            let key_name = memory_manager.load_string(key_idx).to_string();
            if desired.contains(&key_name) {
                new_properties.insert(key_idx, ObjectField::new(val, vis));
            }
        }
    }
    let obj_alloc = memory_manager.allocate_object_with_properties(new_properties);
    Ok(Value::Object(obj_alloc.index))
}

// ─── std.omit ─────────────────────────────────────────────────────────────────

/// std.omit(obj, keys): Return a new object with all fields except those whose names
/// appear in the array keys.
fn std_omit(
    obj_val: Value,
    keys_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let obj_idx = match obj_val {
        Value::Object(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.omit: first argument must be an object".to_string(),
                source_id,
            ));
        }
    };
    let keys_idx = match keys_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.omit: second argument must be an array".to_string(),
                source_id,
            ));
        }
    };
    // Collect excluded key names
    let key_elems = memory_manager.load_array(keys_idx).elements.clone();
    let mut excluded: HashSet<String> = HashSet::new();
    for &k in &key_elems {
        match k {
            Value::String(si) => {
                excluded.insert(memory_manager.load_string(si).to_string());
            }
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.omit: keys must be strings".to_string(),
                    source_id,
                ));
            }
        }
    }
    // Collect visible fields walking the full chain, excluding specified keys
    let all_fields = memory_manager.collect_object_fields_chain(obj_idx);
    let mut new_properties = std::collections::HashMap::new();
    for (key_idx, val, vis) in all_fields {
        if vis != FieldVisibility::Hidden {
            let key_name = memory_manager.load_string(key_idx).to_string();
            if !excluded.contains(&key_name) {
                new_properties.insert(key_idx, ObjectField::new(val, vis));
            }
        }
    }
    let obj_alloc = memory_manager.allocate_object_with_properties(new_properties);
    Ok(Value::Object(obj_alloc.index))
}

// ─── std.product ──────────────────────────────────────────────────────────────

/// std.product(arrs): Cartesian product of an array of arrays.
fn std_product(
    arr_val: Value,
    memory_manager: &mut MemoryManager,
    span: Range<usize>,
    source_id: String,
) -> Result<Value, RuntimeError> {
    let arr_idx = match arr_val {
        Value::Array(i) => i,
        _ => {
            return Err(RuntimeError::new(
                span,
                "std.product: argument must be an array of arrays".to_string(),
                source_id,
            ));
        }
    };
    let sub_arrays: Vec<Value> = memory_manager.load_array(arr_idx).elements.clone();

    // Start with one empty tuple
    let mut result: Vec<Vec<Value>> = vec![vec![]];

    for sub_val in &sub_arrays {
        let sub_idx = match sub_val {
            Value::Array(i) => *i,
            _ => {
                return Err(RuntimeError::new(
                    span,
                    "std.product: each element must be an array".to_string(),
                    source_id,
                ));
            }
        };
        let sub_elems: Vec<Value> = memory_manager.load_array(sub_idx).elements.clone();

        let mut next_result: Vec<Vec<Value>> = Vec::new();
        for existing in &result {
            for &elem in &sub_elems {
                let mut new_tuple = existing.clone();
                new_tuple.push(elem);
                next_result.push(new_tuple);
            }
        }
        result = next_result;
    }

    // Convert Vec<Vec<Value>> to Vec<Value> (each inner Vec becomes an array)
    let mut output: Vec<Value> = Vec::with_capacity(result.len());
    for tuple in result {
        let tuple_alloc = memory_manager.allocate_array(tuple);
        output.push(Value::Array(tuple_alloc.index));
    }
    let alloc = memory_manager.allocate_array(output);
    Ok(Value::Array(alloc.index))
}
