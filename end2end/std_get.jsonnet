assert (
local obj = {a: 1, b:: 2};
std.get(obj, "a", null, true) == 1 &&
std.get(obj, "missing", null, true) == null &&
std.get(obj, "missing", "default", true) == "default" &&
std.get(obj, "b", null, true) == 2 &&
std.get(obj, "b", null, false) == null
); true
