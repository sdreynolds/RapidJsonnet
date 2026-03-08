assert (std.split("foo/_bar", "/_") == ["foo", "bar"] &&
std.split("/_foo/_bar", "/_") == ["", "foo", "bar"] &&
std.split("a,b,c", ",") == ["a", "b", "c"]); true
