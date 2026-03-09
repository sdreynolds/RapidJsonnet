// Test std.sort used as a first-class value with keyF
local mySort = std.sort;
local data = [{ n: "b" }, { n: "a" }, { n: "c" }];
local result = mySort(data, function(x) x.n);
assert result[0].n == "a" : "first should be a";
assert result[1].n == "b" : "second should be b";
assert result[2].n == "c" : "third should be c";

// Also test 1-arg form via value reference
local nums = [3, 1, 2];
local sorted = mySort(nums);
assert sorted == [1, 2, 3] : "numeric sort";
true
