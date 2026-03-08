assert std.objectFromPairs([["a", 1], ["b", 2]]) == { a: 1, b: 2 } : "array pairs";
assert std.objectFromPairs([]) == {} : "empty";
assert std.objectFromPairs([["x", true], ["y", null]]) == { x: true, y: null } : "mixed values";
local obj = { foo: 10, bar: 20 };
assert std.objectFromPairs(
  std.map(function(kv) [kv.key, kv.value], std.objectKeysValues(obj))
) == obj : "round-trip with objectKeysValues";
true
