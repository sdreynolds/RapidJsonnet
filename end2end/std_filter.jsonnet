assert (
std.filter(function(x) x > 2, [1, 2, 3, 4]) == [3, 4] &&
std.filter(function(x) x != null, [1, null, 2, null]) == [1, 2] &&
std.filter(function(x) false, [1, 2, 3]) == []
); true
