assert (
std.manifestJsonEx({b: 2, a: 1}, "  ", "\n", ": ") == "{\n  \"a\": 1,\n  \"b\": 2\n}" &&
std.manifestJsonEx([1, 2], "  ", "\n", ": ") == "[\n  1,\n  2\n]" &&
std.manifestJsonEx(null, "  ", "\n", ": ") == "null" &&
std.manifestJsonEx({a: 1}, "", "", ":") == "{\"a\":1}"
); true
