// Isolates the AddConst fused-opcode optimization: a tight fold doing
// `acc + 1` (accumulator + a bare numeric literal) many times.
std.foldl(function(acc, _) acc + 1, std.range(1, 200000), 0)
