assert std.manifestPython(null) == "None" : "null is None";
assert std.manifestPython(true) == "True" : "true is True";
assert std.manifestPython(false) == "False" : "false is False";
assert std.manifestPython(42) == "42" : "integer";
assert std.manifestPython("hello") == '"hello"' : "string";
assert std.manifestPython([1, true, null]) == "[\n   1,\n   True,\n   None\n]" : "array";

// Object value
local obj_result = std.manifestPython({a: 1, b: true});
assert std.findSubstr('"a":', obj_result) != [] : "object key a";
assert std.findSubstr("True", obj_result) != [] : "True in object";
assert std.manifestPython({}) == "{ }" : "empty object";
true
