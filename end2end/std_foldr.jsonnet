std.foldr(function(x, acc) acc + x, [1, 2, 3, 4], 0) == 10 &&
std.foldr(function(x, acc) [x] + acc, [1, 2, 3], []) == [1, 2, 3] &&
std.foldr(function(x, acc) acc, [], 99) == 99 &&
std.foldr(function(x, acc) x - acc, [1, 2, 3], 0) == 2
