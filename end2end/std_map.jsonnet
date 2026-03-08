assert (
std.map(function(x) x * 2, [1, 2, 3]) == [2, 4, 6] &&
std.map(function(x) x + "!", ["a", "b"]) == ["a!", "b!"] &&
std.map(function(x) x, []) == []
); true
