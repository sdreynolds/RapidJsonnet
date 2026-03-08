assert (
local obj = {a: 1, b: 2};
local kv = std.objectKeysValues(obj);
kv[0].key == "a" &&
kv[0].value == 1 &&
kv[1].key == "b" &&
kv[1].value == 2
); true
