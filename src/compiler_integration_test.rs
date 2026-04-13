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

#[cfg(test)]
mod integration_tests {
    use crate::Compiler;

    fn assert_compiles(source: &str) {
        let mut sc = scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "test.jsonnet");
        let chunk = compiler
            .compile(&mut memory_manager)
            .expect("compile failed");
        assert!(!chunk.is_empty());
    }

    fn compile_err(source: &str) -> String {
        let mut sc = scanner::Scanner::new(source, "test.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "test.jsonnet");
        compiler.compile(&mut memory_manager).unwrap_err().message
    }

    #[test]
    fn test_simple_number_file() {
        let mut sc = scanner::Scanner::new("42", "simple_number.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "simple_number.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
    }

    #[test]
    fn test_simple_addition_file() {
        let mut sc = scanner::Scanner::new("3 + 4", "simple_addition.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "simple_addition.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 2);
    }

    #[test]
    fn test_unary_minus_file() {
        let mut sc = scanner::Scanner::new("-42", "unary_minus.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "unary_minus.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        assert_eq!(chunk.constants.len(), 1);
    }

    #[test]
    fn test_complex_expression_file() {
        let mut sc = scanner::Scanner::new("-3 * (4 - 1 + 2)", "complex_expression.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "complex_expression.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have constants: 3, 4, 1, 2
        assert_eq!(chunk.constants.len(), 4);
    }

    #[test]
    fn test_precedence_test_file() {
        let mut sc = scanner::Scanner::new("(-1 + 2) * 3 - -4", "precedence_test.jsonnet");
        let mut memory_manager = memory_manager::MemoryManager::new();
        let compiler = Compiler::new(&mut sc, "precedence_test.jsonnet");
        let chunk = compiler.compile(&mut memory_manager).unwrap();

        // Should have constants: 1, 2, 3, 4
        assert_eq!(chunk.constants.len(), 4);
    }

    // --- New integration tests ---

    #[test]
    fn test_object_literal_compilation() {
        assert_compiles("{a: 1, b: 2}");
    }

    #[test]
    fn test_object_hidden_field_compilation() {
        assert_compiles("{a:: 1}");
    }

    #[test]
    fn test_object_forced_field_compilation() {
        assert_compiles("{a::: 1}");
    }

    #[test]
    fn test_object_override_syntax_compilation() {
        // +: is a field merge operator; test verifies the compiler accepts the syntax
        assert_compiles("{a: 1} + {a+: 10}");
    }

    #[test]
    fn test_object_comprehension_compilation() {
        assert_compiles("{[k]: 1 for k in [\"a\",\"b\"]}");
    }

    #[test]
    fn test_assert_compilation() {
        assert_compiles("assert true; 1");
    }

    #[test]
    fn test_assert_with_message_compilation() {
        assert_compiles("assert 1 == 1 : \"fail\"; true");
    }

    #[test]
    fn test_function_with_defaults_compilation() {
        assert_compiles("local f(x, y=10) = x + y; f(5)");
    }

    // tailstrict is not yet implemented in the compiler; skipped.
    // #[test]
    // fn test_tailstrict_compilation() {
    //     assert_compiles("local f(n) = if n == 0 then 0 else tailstrict f(n-1); f(5)");
    // }

    #[test]
    fn test_self_compilation() {
        assert_compiles("{a: 1, b: self.a}");
    }

    #[test]
    fn test_super_compilation() {
        assert_compiles("{x:1} + {y: super.x + 1}");
    }

    #[test]
    fn test_dollar_compilation() {
        assert_compiles("{a: 1, b: $.a}");
    }

    #[test]
    fn test_import_compilation() {
        // Import resolution happens at runtime; compilation itself should succeed
        assert_compiles("import \"nonexistent.jsonnet\"");
    }

    #[test]
    fn test_importstr_compilation() {
        assert_compiles("importstr \"nonexistent.txt\"");
    }

    #[test]
    fn test_importbin_compilation() {
        assert_compiles("importbin \"nonexistent.bin\"");
    }

    #[test]
    fn test_conditional_compilation() {
        assert_compiles("if true then 1 else 2");
    }

    #[test]
    fn test_error_expr_compilation() {
        assert_compiles("if false then error \"boom\" else 1");
    }

    #[test]
    fn test_array_comprehension_filter_compilation() {
        assert_compiles("[x for x in [1,2,3] if x > 1]");
    }

    #[test]
    fn test_compile_err_invalid_syntax() {
        let msg = compile_err("local = 1;");
        assert!(
            msg.contains("identifier") || msg.contains("Expected") || msg.contains("unexpected"),
            "unexpected message: {}",
            msg
        );
    }

    // Gap-fill tests for uncovered compiler paths

    #[test]
    fn test_object_with_assert_compilation() {
        assert_compiles("{x: 1, assert self.x == 1}");
    }

    #[test]
    fn test_object_with_assert_and_message_compilation() {
        assert_compiles(r#"{x: 1, assert self.x == 1 : "x must be 1"}"#);
    }

    #[test]
    fn test_object_with_local_compilation() {
        assert_compiles("{local n = 5, x: n + 1}");
    }

    #[test]
    fn test_unary_plus_compilation() {
        assert_compiles("+5");
    }

    #[test]
    fn test_bitnot_compilation() {
        assert_compiles("~5");
    }

    #[test]
    fn test_in_operator_compilation() {
        assert_compiles(r#""a" in {a: 1}"#);
    }

    #[test]
    fn test_super_has_field_compilation() {
        assert_compiles("local b = {x: 1}; (b + {y: 'x' in super}).y");
    }

    #[test]
    fn test_super_bracket_syntax_compilation() {
        assert_compiles(r#"local b = {x: 1}; (b + {y: super["x"]}).y"#);
    }

    #[test]
    fn test_importstr_error_missing_path() {
        let msg = compile_err("importstr");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_importbin_gapfill_compilation() {
        // importbin with a string literal should compile (even if execution would fail)
        assert_compiles(r#"importbin "some_file.bin""#);
    }

    #[test]
    fn test_computed_field_name_compilation() {
        assert_compiles(r#"local k = "x"; {[k]: 1}"#);
    }

    #[test]
    fn test_object_with_trailing_comma_compilation() {
        assert_compiles("{a: 1, b: 2,}");
    }

    #[test]
    fn test_function_with_many_params_compilation() {
        assert_compiles("local f(a, b, c, d) = a + b + c + d; f(1, 2, 3, 4)");
    }

    #[test]
    fn test_nested_function_compilation() {
        assert_compiles("local f = function(x) function(y) x + y; f(1)(2)");
    }

    #[test]
    fn test_object_in_array_comprehension_compilation() {
        assert_compiles("[{x: i} for i in [1, 2, 3]]");
    }

    #[test]
    fn test_string_key_field_compilation() {
        assert_compiles(r#"{"hello world": 42}"#);
    }

    #[test]
    fn test_multiple_local_compilation() {
        assert_compiles("local a = 1; local b = 2; local c = 3; a + b + c");
    }

    #[test]
    fn test_deep_nested_function_calls_compilation() {
        assert_compiles("std.length(std.filter(function(x) x > 0, [1, -1, 2]))");
    }

    #[test]
    fn test_error_expr_with_format_compilation() {
        assert_compiles(r#"if true then 1 else error "bad value""#);
    }

    #[test]
    fn test_object_field_override_plus_compilation() {
        assert_compiles("{a: 1} + {a+: 10}");
    }

    #[test]
    fn test_multiple_inheritance_compilation() {
        assert_compiles("local a = {x: 1}; local b = a + {y: 2}; b + {z: 3}");
    }

    #[test]
    fn test_dollar_sign_compilation() {
        assert_compiles("{x: 1, y: $.x + 1}");
    }

    #[test]
    fn test_tail_call_compilation() {
        assert_compiles("local f(n, acc) = if n == 0 then acc else f(n - 1, acc + n); f(10, 0)");
    }

    #[test]
    fn test_object_comprehension_with_condition_compilation() {
        assert_compiles(r#"{[k]: 1 for k in ["a", "b", "c"] if k != "b"}"#);
    }

    #[test]
    fn test_slice_with_step_compilation() {
        assert_compiles("[1, 2, 3, 4, 5][::2]");
    }

    #[test]
    fn test_named_arg_call_compilation() {
        assert_compiles("std.substr('hello', from=1, len=3)");
    }

    #[test]
    fn test_super_dot_field_missing_name_error() {
        let msg = compile_err("local b = {x: 1}; (b + {y: super.}).y");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_import_non_string_error() {
        let msg = compile_err("import 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_importstr_non_string_error() {
        let msg = compile_err("importstr 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_importbin_non_string_error() {
        let msg = compile_err("importbin 42");
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_assert_expr_compilation() {
        assert_compiles("assert 1 == 1; true");
    }

    #[test]
    fn test_assert_with_msg_compilation() {
        assert_compiles(r#"assert 1 == 1 : "should be equal"; true"#);
    }

    #[test]
    fn test_new_from_file_nonexistent() {
        let result = crate::Compiler::new_from_file("nonexistent_file_xyz.jsonnet");
        assert!(result.is_err());
    }

    // Gap-fill: upvalue resolution across nested closure scopes

    #[test]
    fn test_closure_upvalue_compilation() {
        assert_compiles("local x = 10; local f() = x; f()");
    }

    #[test]
    fn test_nested_closure_compilation() {
        assert_compiles("local outer(x) = local inner() = x; inner; outer(5)()");
    }

    #[test]
    fn test_nested_closure_upvalue_resolution() {
        // A closure inside another closure — forces resolve_upvalue to recurse into enclosing scope
        assert_compiles("local outer(x) = local inner(y) = x + y; inner; outer(1)(2)");
    }

    #[test]
    fn test_deeply_nested_upvalue() {
        // Three levels of nesting to exercise the chained upvalue path
        assert_compiles("local f(a) = local g(b) = local h(c) = a + b + c; h; g; f(1)(2)(3)");
    }
}
