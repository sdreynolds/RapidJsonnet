std.splitLimit("/_foo/_bar", "/_", 1) == ["", "foo/_bar"] &&
std.splitLimit("a,b,c,d", ",", 2) == ["a", "b", "c,d"] &&
std.splitLimit("a,b,c", ",", -1) == ["a", "b", "c"] &&
std.splitLimit("a,b,c", ",", 0) == ["a,b,c"]
