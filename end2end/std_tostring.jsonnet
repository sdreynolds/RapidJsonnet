assert std.toString(null) == "null" : "null";
assert std.toString(true) == "true" : "true";
assert std.toString(false) == "false" : "false";
assert std.toString(42) == "42" : "number";
assert std.toString(3.14) == "3.14" : "float";
assert std.toString("hello") == "hello" : "string";
assert std.toString({a: 1}) == "{\"a\": 1}" : "object toString";
assert std.toString([1, 2, 3]) == "[1, 2, 3]" : "array toString";
assert std.toString({}) == "{ }" : "empty object toString";
assert std.toString([]) == "[ ]" : "empty array toString";
true
