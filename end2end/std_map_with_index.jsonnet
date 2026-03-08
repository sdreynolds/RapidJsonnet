assert (
std.mapWithIndex(function(i, x) i, ["a", "b", "c"]) == [0, 1, 2] &&
std.mapWithIndex(function(i, x) std.toString(i) + ": " + x, ["a", "b"]) == ["0: a", "1: b"]
); true
