local obj = {a: 1, b:: 2, c: 3};
local vals = std.objectValuesAll(obj);
std.length(vals) == 3 &&
std.contains(vals, 1) &&
std.contains(vals, 2) &&
std.contains(vals, 3)
