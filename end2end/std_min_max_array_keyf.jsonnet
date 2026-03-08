// keyF form
assert std.minArray([{v:3},{v:1},{v:2}], function(x) x.v) == {v:1} : "min keyF";
assert std.maxArray([{v:3},{v:1},{v:2}], function(x) x.v) == {v:3} : "max keyF";
// onEmpty form
assert std.minArray([], null, "fallback") == "fallback" : "min onEmpty";
assert std.maxArray([], null, 99) == 99 : "max onEmpty";
// Negative numbers still work
assert std.minArray([-1, -2, -3]) == -3 : "min negatives";
assert std.maxArray([-1, -2, -3]) == -1 : "max negatives";
true
