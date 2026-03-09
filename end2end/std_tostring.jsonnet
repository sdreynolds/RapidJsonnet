assert std.toString(null) == "null" : "null";
assert std.toString(true) == "true" : "true";
assert std.toString(false) == "false" : "false";
assert std.toString(42) == "42" : "number";
assert std.toString(3.14) == "3.14" : "float";
assert std.toString("hello") == "hello" : "string";
assert std.toString({a: 1}) == "{\n   \"a\": 1\n}" : "object toString";
assert std.toString([1, 2, 3]) == "[\n   1,\n   2,\n   3\n]" : "array toString";
assert std.toString({}) == "{ }" : "empty object toString";
assert std.toString([]) == "[ ]" : "empty array toString";
true
