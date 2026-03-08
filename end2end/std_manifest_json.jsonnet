assert (
std.manifestJson(null) == "null" &&
std.manifestJson(true) == "true" &&
std.manifestJson(false) == "false" &&
std.manifestJson(42) == "42" &&
std.manifestJson(3.14) == "3.14" &&
std.manifestJson("hi") == "\"hi\"" &&
std.manifestJson([]) == "[ ]" &&
std.manifestJson({}) == "{ }" &&
std.manifestJson([1, 2, 3]) == "[\n   1,\n   2,\n   3\n]" &&
std.manifestJson({b: 2, a: 1}) == "{\n   \"a\": 1,\n   \"b\": 2\n}"
); true
