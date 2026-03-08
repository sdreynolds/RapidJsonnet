assert (
std.flatMap(function(x) [x, x], [1, 2, 3]) == [1, 1, 2, 2, 3, 3] &&
std.flatMap(function(x) if x == 2 then [] else [x], [1, 2, 3]) == [1, 3]
); true
