assert std.mapKeys(function(k) std.asciiUpper(k), { foo: 1, bar: 2 })
  == { BAR: 2, FOO: 1 } : "uppercase keys";
assert std.mapKeys(function(k) k + "_x", { a: 10, b: 20 })
  == { a_x: 10, b_x: 20 } : "suffix keys";
assert std.mapKeys(function(k) k, {}) == {} : "empty object";
true
