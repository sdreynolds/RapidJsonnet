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
        let mut sc =
            scanner::Scanner::new("(-1 + 2) * 3 - -4", "precedence_test.jsonnet");
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
        assert!(!msg.is_empty());
    }
}
