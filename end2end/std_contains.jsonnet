std.contains([1, 2, 3], 2) == true &&
std.contains([1, 2, 3], 5) == false &&
std.contains([], 1) == false &&
std.contains(["a", "b"], "a") == true &&
std.contains([null, true], null) == true
