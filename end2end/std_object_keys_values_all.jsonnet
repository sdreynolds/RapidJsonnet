local o = { a: 1, b:: 2, c: 3 };
local kvs = std.objectKeysValuesAll(o);
std.length(kvs) == 3 &&
kvs[0].key == "a" && kvs[0].value == 1 &&
kvs[1].key == "b" && kvs[1].value == 2 &&
kvs[2].key == "c" && kvs[2].value == 3
