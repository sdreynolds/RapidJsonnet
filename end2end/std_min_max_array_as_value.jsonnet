// Test std.minArray and std.maxArray used as first-class values
local myMin = std.minArray;
local myMax = std.maxArray;
assert myMin([3, 1, 2]) == 1 : "minArray basic";
assert myMax([3, 1, 2]) == 3 : "maxArray basic";
// keyF form
local data = [{ v: 3 }, { v: 1 }, { v: 2 }];
assert myMin(data, function(x) x.v) == { v: 1 } : "minArray with keyF";
assert myMax(data, function(x) x.v) == { v: 3 } : "maxArray with keyF";
true
