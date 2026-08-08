// Exercises the ConcatConst fused opcode: both operands of `+` are statically
// known to be String and the right operand is a bare string literal.
local greeting = ("hello" + " ") + "world";
assert greeting == "hello world";
local chained = "a" + "b" + "c" + "d";
assert chained == "abcd";
greeting
