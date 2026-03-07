std.manifestJsonMinified(null) == "null" &&
std.manifestJsonMinified(true) == "true" &&
std.manifestJsonMinified(false) == "false" &&
std.manifestJsonMinified(42) == "42" &&
std.manifestJsonMinified(3.14) == "3.14" &&
std.manifestJsonMinified("hi") == "\"hi\"" &&
std.manifestJsonMinified([1, 2, 3]) == "[1,2,3]" &&
std.manifestJsonMinified({b: 2, a: 1}) == "{\"a\":1,\"b\":2}"
