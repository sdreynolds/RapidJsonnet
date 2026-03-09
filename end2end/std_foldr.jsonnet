assert std.foldr(function(x, acc) acc + x, [1, 2, 3, 4], 0) == 10 : "sum";
// Subtraction proves f(elem, acc) order: f(3,0)=3, f(2,3)=-1, f(1,-1)=2
assert std.foldr(function(x, acc) x - acc, [1, 2, 3], 0) == 2 : "order verification";
assert std.foldr(function(x, acc) [x] + acc, [1, 2, 3], []) == [1, 2, 3] : "array build";
assert std.foldr(function(x, acc) acc, [], 99) == 99 : "empty returns init";
assert std.foldr(function(x, acc) x + acc, [42], 0) == 42 : "single element";
assert std.foldr(function(x, acc) x + acc, ["a", "b", "c"], "") == "abc" : "string concat";
true
