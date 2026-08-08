// Isolates the ConcatConst fused-opcode optimization. ConcatConst only fires
// when BOTH operands of `+` are statically known to be String at compile time
// (see compiler.rs's ExpressionType tracking) - a loop accumulator like `acc`
// has an Unknown static type (locals/params aren't type-tracked), so
// `acc + 'x'` fuses into AddConst instead, not ConcatConst. To exercise
// ConcatConst specifically in a hot loop, chain bare string literals together
// on every iteration - each `+` in the chain below has both operands
// statically known String, so it fuses into ConcatConst.
std.foldl(
  function(acc, _) acc + (('a' + 'b') + ('c' + 'd')),
  std.range(1, 20000),
  ''
)
