assert (std.slice([1, 2, 3, 4, 5, 6], 0, 4, 1) == [1, 2, 3, 4] &&
std.slice([1, 2, 3, 4, 5, 6], 1, 6, 2) == [2, 4, 6] &&
std.slice("jsonnet", 0, 4, 1) == "json" &&
std.slice("jsonnet", -3, null, null) == "net" &&
std.slice([1, 2, 3, 4, 5], null, 3, null) == [1, 2, 3]); true
