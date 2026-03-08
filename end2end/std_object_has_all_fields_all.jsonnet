assert (
local obj = {a: 1, b:: 2};
std.objectHasAll(obj, "a") == true &&
std.objectHasAll(obj, "b") == true &&
std.objectHasAll(obj, "z") == false &&
std.objectFieldsAll(obj) == ["a", "b"]
); true
