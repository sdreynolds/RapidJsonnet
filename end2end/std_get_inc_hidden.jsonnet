// Test std.get with inc_hidden parameter
local obj = { a: 1, b:: 2 };
assert std.get(obj, "a") == 1 : "visible field default";
assert std.get(obj, "a", null, true) == 1 : "visible with inc_hidden=true";
assert std.get(obj, "b", "default", false) == "default" : "hidden with inc_hidden=false";
assert std.get(obj, "b", null, true) == 2 : "hidden with inc_hidden=true";
assert std.get(obj, "missing", 99) == 99 : "missing field returns default";
true
