# Compiler
A `Compiler` struct will be handed either a string contents of a jsonnet file or a filename to open and read the contents in. With those contents, it will create a `Scanner` and then create a `Parser` using that `Scanner`. The compiler will be a single pass using Pratt Parser for jsonnet. The compiler will use the `Parser` to step through the token stream. The `Compiler` will create a new `Chunk` as it's `compiling_chunk` and when the `Compiler` reaches the last `Token` from the `Parser` it will emit the `Return` `opcode` into the `compiling_chunk`. In this compiler we are going to use a Pratt compiler and for this task we will focus on handling
- `Postfix`,
- `Unary Prefix`,
- `Exponentiation`,
- `Addition/Subtraction`
- `Comparison`
- `Bitwise Operators`

Leaving `Logical And`, `Logical Or`, `Ternary Operator` and `Object Composition` for a later task. Keeping the task small is the name of the game.

#### Compiler struct
The compiler will have the `compiling_chunk: Chunk` and a `parser: Parser` as fields and has two constructors

- `new(input: &str, source_id &str)`
- `newFromFile(fileName: &str)` -- `fileName` will be the `source_id` and it will need to read the contents of the file into an `input` and pass that into the `Scanner` it creates for the `Parser`

#### Method Signatures
```rust
impl Compiler {
    pub fn new(input: &str, source_id: &str) -> Self
    pub fn new_from_file(file_name: &str) -> Result<Self, std::io::Error>
    pub fn compile(mut self) -> Result<Chunk, CompilerError>
    fn parse_expr(&mut self, min_bp: u8) -> Result<(), CompilerError>
    fn parse_prefix(&mut self) -> Result<(), CompilerError>
    fn parse_infix(&mut self, left_bp: u8) -> Result<(), CompilerError>
    fn advance(&mut self) -> Result<(), CompilerError>
    fn emit_opcode(&mut self, opcode: Opcode)
    fn emit_constant(&mut self, value: f64) -> Result<u16, CompilerError>
    fn get_binding_power(&self, token: &Token) -> Option<(u8, u8)> // (lbp, rbp)
}
```

### Pratt Parsing in Rust: Theory to Code
Table 1: Jsonnet Operator Precedence and Associativity
| Operator Family         | Symbol(s)            | Arity        | Precedence Level (Numerical) | Associativity   |
|-------------------------|----------------------|--------------|:----------------------------:|:----------------|
| Postfix                 | [...], (...), .      | Postfix      | 90                           | Left            |
| Unary Prefix            | !, ~, -, +           | Prefix       | 80                           | Right           |
| Exponentiation          | ^                    | Binary Infix | 70                           | Right           |
| Multiplication/Division | *, /                 | Binary Infix | 60                           | Left            |
| Addition/Subtraction    | +, -                 | Binary Infix | 50                           | Left            |
| Comparison              | ==, !=, <, <=, >, >= | Binary Infix | 40                           | Non-associative |
| Bitwise Operators       | &, ^,                | Binary Infix | 30, 25, 20                   | Left            |
| Logical AND             | &&                   | Binary Infix | 15                           | Left            |
| Logical OR              |                      | Binary Infix | 15                           | Left            |
| Ternary Operator        | ? :                  | Mixfix       | 5                            | Right           |
| Object Composition      | + (on objects)       | Binary Infix | 50                           | Left            |

#### Precedence Constants
```rust
const PRECEDENCE_POSTFIX: u8 = 90;
const PRECEDENCE_UNARY: u8 = 80;
const PRECEDENCE_EXPONENTIATION: u8 = 70;
const PRECEDENCE_MULTIPLICATIVE: u8 = 60;
const PRECEDENCE_ADDITIVE: u8 = 50;
const PRECEDENCE_COMPARISON: u8 = 40;
const PRECEDENCE_BITAND: u8 = 30;
const PRECEDENCE_BITXOR: u8 = 25;
const PRECEDENCE_BITOR: u8 = 20;
const PRECEDENCE_LOGICAL_AND: u8 = 15;
const PRECEDENCE_LOGICAL_OR: u8 = 10;
const PRECEDENCE_TERNARY: u8 = 5;
```



#### The Core Pratt Algorithm Demystified

The theoretical foundation of Pratt parsing is simple yet powerful, and its implementation in Rust is a matter of translating these core concepts into code. The central component is a single, recursive function, which can be named parse_expr. This function takes a single argument,

min_bp (minimum binding power), which is the binding power of the operator to the left of the current expression.

The algorithm proceeds in two distinct phases within a while loop:

    Parse the left-hand side: The parser first calls a helper function to handle the nud of the next token, which initializes the left expression. This phase handles primary expressions (like numbers or variables) or unary prefix operators.

    Loop for infix/postfix operators: The parser then enters a loop. In each iteration, it peeks at the next token and retrieves its left binding power (lbp). If this lbp is less than or equal to the min_bp passed into the function, the loop terminates. This is the "it stops" condition that prevents the parser from consuming an operator that binds less tightly than the one that came before it. If the

    lbp is greater, the parser consumes the operator and calls its led handler, which recursively calls parse_expr to get the right-hand side of the expression. The result is then combined into a new AST node, which becomes the new

    left for the next loop iteration.

This mechanism correctly handles both precedence and associativity. The recursive call for the right-hand side is initiated with a new min_bp, which is the current operator's right binding power (rbp). A subtle but critical detail lies in how this

rbp is set to handle different associativities. For a left-associative operator like addition (+), the rbp is typically set equal to its lbp (or lbp + 1 to prevent endless recursion on same-precedence operators). This ensures that an expression like A + B + C is parsed as (A + B) + C, where the first addition is reduced before the second. The recursive call for the second

+ will receive a min_bp that is strong enough to stop it from consuming the C first, correctly forcing the left-to-right evaluation. Conversely, for a right-associative operator like exponentiation (^), the rbp must be set to lbp + 1. This forces the recursive call for the right-hand side to consume subsequent operators of the same precedence first, correctly parsing A ^ B ^ C as A ^ (B ^ C). A precise understanding of this asymmetry is crucial for a correct and robust implementation.

### What tokens to target for the first task
For the first task we are targeting a small selection of tokens and expressions. In this task we are going to parse and compile

- grouping expressions <example>(1 + 3) * 4</example>
- binary expressions <example> 7 / 10</example>
- unary expressions <example>-4</example>
- number expressions <example>100.34</example>

### Pratt Parser to Chunk
The Pratt Parser written into the compiler should take these tokens and expressions and write the matching `opcode` and `operand` required for that. The goal for this task is to handle simple jsonnet code

<example>
-3 * (4 - 1 + 2)
</example>
<example>
(-1 + 2) * 3 - -4
</example>


### Error handling
The compiler will return a `CompilerError` when the token cannot be parsed, or when the `Token` doesn't make sense in the context. The `CompilerError` should be a type defintion alias to `ScannerError` to make it easy. The `span` for the `ScannerError` can be taken from the `TokenInfo` returned by the `Parser`

#### Error Categories
```rust
pub type CompilerError = ScanError;

impl CompilerError {
    pub fn unexpected_token(token: &TokenInfo, expected: &str) -> Self {
        ScanError {
            span: token.span.clone(),
            message: format!("Expected {}, found {:?}", expected, token.token_type),
        }
    }
    
    pub fn unexpected_eof(span: Range<usize>) -> Self {
        ScanError {
            span,
            message: "Unexpected end of input".to_string(),
        }
    }
    
    pub fn invalid_expression(token: &TokenInfo) -> Self {
        ScanError {
            span: token.span.clone(),
            message: format!("Invalid expression starting with {:?}", token.token_type),
        }
    }
    
    pub fn too_many_constants() -> Self {
        ScanError {
            span: 0..0,
            message: "Too many constants (maximum 65535)".to_string(),
        }
    }
}
```

### Integration points
The `Compiler` will use the `Parser` to keep track of the `previous_token` and the `current_token` and to check if the `parser` `had_error`.

The `Compiler` will insert the constants into the `constants` vector and then specify that index as the `u16` for the `LoadConst` `opcode` `operand`.


### End to End Testing
Create a `end2end` directory with several simple `jsonnet` valid files and run them through a `Compiler` to test that they parse and produce valid `Chunk` objects. Keep the examples simple for now as we ramp up a comprehsive testing strategy.
