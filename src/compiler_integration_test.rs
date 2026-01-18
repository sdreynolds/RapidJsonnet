use crate::compiler::Compiler;
use std::fs;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_number_file() {
        let mut scanner = crate::scanner::Scanner::new("42", "simple_number.jsonnet");
        let compiler = Compiler::new(&mut scanner, "simple_number.jsonnet");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
    }

    #[test]
    fn test_simple_addition_file() {
        let mut scanner = crate::scanner::Scanner::new("3 + 4", "simple_addition.jsonnet");
        let compiler = Compiler::new(&mut scanner, "simple_addition.jsonnet");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], 3.0);
        assert_eq!(chunk.constants[1], 4.0);
    }

    #[test]
    fn test_unary_minus_file() {
        let mut scanner = crate::scanner::Scanner::new("-42", "unary_minus.jsonnet");
        let compiler = Compiler::new(&mut scanner, "unary_minus.jsonnet");
        let chunk = compiler.compile().unwrap();

        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
    }

    #[test]
    fn test_complex_expression_file() {
        let mut scanner =
            crate::scanner::Scanner::new("-3 * (4 - 1 + 2)", "complex_expression.jsonnet");
        let compiler = Compiler::new(&mut scanner, "complex_expression.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should have constants: 3, 4, 1, 2
        assert_eq!(chunk.constants.len(), 4);
        assert_eq!(chunk.constants[0], 3.0);
        assert_eq!(chunk.constants[1], 4.0);
        assert_eq!(chunk.constants[2], 1.0);
        assert_eq!(chunk.constants[3], 2.0);
    }

    #[test]
    fn test_precedence_test_file() {
        let mut scanner =
            crate::scanner::Scanner::new("(-1 + 2) * 3 - -4", "precedence_test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "precedence_test.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should have constants: 1, 2, 3, 4
        assert_eq!(chunk.constants.len(), 4);
        assert_eq!(chunk.constants[0], 1.0);
        assert_eq!(chunk.constants[1], 2.0);
        assert_eq!(chunk.constants[2], 3.0);
        assert_eq!(chunk.constants[3], 4.0);
    }

    #[test]
    fn test_function_with_no_parameters() {
        let mut scanner = crate::scanner::Scanner::new("function() { 42 }", "test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should have one constant: 42
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
    }

    #[test]
    fn test_function_with_parameters() {
        let mut scanner = crate::scanner::Scanner::new("function(x, y) { x + y }", "test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should compile without error
        assert!(chunk.bytecode.len() > 0);
    }

    #[test]
    fn test_function_with_defaults() {
        let mut scanner =
            crate::scanner::Scanner::new("function(x, y = 10) { x + y }", "test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should compile without error - defaults are parsed and tracked
        assert!(chunk.bytecode.len() > 0);
        assert_eq!(chunk.constants.len(), 2); // 10 and the function itself (or just 10)
    }

    #[test]
    fn test_function_with_all_defaults() {
        let mut scanner =
            crate::scanner::Scanner::new("function(x = 5, y = 10) { x + y }", "test.jsonnet");
        let compiler = Compiler::new(&mut scanner, "test.jsonnet");
        let chunk = compiler.compile().unwrap();

        // Should compile without error
        assert!(chunk.bytecode.len() > 0);
        assert_eq!(chunk.constants.len(), 2); // 5 and 10
    }
}
