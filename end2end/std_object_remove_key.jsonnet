assert (
local obj = { a: 1, b: 2, c: 3 };
local result = std.objectRemoveKey(obj, "b");

std.objectFields(result) == ["a", "c"] &&
result.a == 1 &&
result.c == 3 &&
std.objectFields(std.objectRemoveKey(obj, "z")) == ["a", "b", "c"] &&
std.objectFields(std.objectRemoveKey({}, "x")) == []
); true
