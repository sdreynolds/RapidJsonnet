// Exercises the AddConst fused opcode: `<value> + <literal>` where the value
// is not itself a compile-time constant.
local x = 41;
local y = x + 1;
local acc = std.foldl(function(a, i) a + 1, std.range(1, 1000), 0);
assert y == 42;
assert acc == 1000;
assert (x + "!") == "41!";
{ y: y, acc: acc }
