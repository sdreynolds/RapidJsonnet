local obj = {a: 1, b: 2};
local pairs = stdExtended.toPairs(obj);
assert std.length(pairs) == 2 : "length 2";
local sorted = std.sort(pairs, function(p) p[0]);
assert sorted == [["a", 1], ["b", 2]] : "sorted pairs";
assert stdExtended.toPairs({}) == [] : "empty object";
assert stdExtended.objectFromPairs(stdExtended.toPairs({x: 10, y: 20})) == {x: 10, y: 20} : "round-trip";
true
