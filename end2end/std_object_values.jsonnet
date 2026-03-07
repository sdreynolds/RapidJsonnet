local obj = {a: 1, b: 2, c: 3};
local computed = {x: 1 + 1, y: 3 * 2};
std.objectValues(obj) == [1, 2, 3] &&
std.objectValues(computed) == [2, 6]
