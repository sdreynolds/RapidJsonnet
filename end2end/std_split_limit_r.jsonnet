std.splitLimitR("/_foo/_bar", "/_", 1) == ["/_foo", "bar"] &&
std.splitLimitR("a,b,c,d", ",", 2) == ["a,b", "c", "d"] &&
std.splitLimitR("a,b,c", ",", -1) == ["a", "b", "c"]
