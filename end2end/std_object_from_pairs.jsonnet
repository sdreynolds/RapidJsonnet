assert stdExtended.objectFromPairs([["a", 1], ["b", 2]]) == { a: 1, b: 2 } : "array pairs";
assert stdExtended.objectFromPairs([]) == {} : "empty";
assert stdExtended.objectFromPairs([["x", true], ["y", null]]) == { x: true, y: null } : "mixed values";
local obj = { foo: 10, bar: 20 };
assert stdExtended.objectFromPairs(
  std.map(function(kv) [kv.key, kv.value], std.objectKeysValues(obj))
) == obj : "round-trip with objectKeysValues";
true
