assert std.filterObject(function(k, v) v > 1, { a: 1, b: 2, c: 3 })
  == { b: 2, c: 3 } : "filter by value";
assert std.filterObject(function(k, v) std.startsWith(k, "a"), { apple: 1, banana: 2, avocado: 3 })
  == { apple: 1, avocado: 3 } : "filter by key prefix";
assert std.filterObject(function(k, v) false, { x: 1 }) == {} : "filter all out";
assert std.filterObject(function(k, v) true, {}) == {} : "empty object";
true
