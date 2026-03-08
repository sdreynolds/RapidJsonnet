assert (
std.foldl(function(acc, x) acc + x, [1, 2, 3, 4], 0) == 10 &&
std.foldl(function(acc, x) acc + [x], [1, 2, 3], []) == [1, 2, 3] &&
std.foldl(function(acc, x) acc, [], 42) == 42
); true
