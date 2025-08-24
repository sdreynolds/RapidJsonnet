use std::fs;
use crate::compiler::Compiler;

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_simple_number_file() {
        let compiler = Compiler::new("42", "simple_number.jsonnet");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
    }

    #[test]
    fn test_simple_addition_file() {
        let compiler = Compiler::new("3 + 4", "simple_addition.jsonnet");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 2);
        assert_eq!(chunk.constants[0], 3.0);
        assert_eq!(chunk.constants[1], 4.0);
    }

    #[test]
    fn test_unary_minus_file() {
        let compiler = Compiler::new("-42", "unary_minus.jsonnet");
        let chunk = compiler.compile().unwrap();
        
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], 42.0);
    }

    #[test]
    fn test_complex_expression_file() {
        let compiler = Compiler::new("-3 * (4 - 1 + 2)", "complex_expression.jsonnet");
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
        let compiler = Compiler::new("(-1 + 2) * 3 - -4", "precedence_test.jsonnet");
        let chunk = compiler.compile().unwrap();
        
        // Should have constants: 1, 2, 3, 4
        assert_eq!(chunk.constants.len(), 4);
        assert_eq!(chunk.constants[0], 1.0);
        assert_eq!(chunk.constants[1], 2.0);
        assert_eq!(chunk.constants[2], 3.0);
        assert_eq!(chunk.constants[3], 4.0);
    }
}