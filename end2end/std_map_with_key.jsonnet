assert (
local obj = { a: 1, b: 2, c: 3 };
local mapped = std.mapWithKey(function(k, v) k + '=' + std.toString(v), obj);
mapped.a == 'a=1' &&
mapped.b == 'b=2' &&
mapped.c == 'c=3' &&
std.mapWithKey(function(k, v) v * 2, { x: 5 }).x == 10 &&
std.length(std.mapWithKey(function(k, v) v, {})) == 0
); true
