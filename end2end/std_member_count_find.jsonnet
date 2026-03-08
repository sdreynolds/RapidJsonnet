assert (
std.member([1, 2, 3], 2) == true &&
std.member([1, 2, 3], 5) == false &&
std.member("foobar", "oo") == true &&
std.member("foobar", "zz") == false &&
std.count([1, 2, 1, 3, 1], 1) == 3 &&
std.count([1, 2, 3], 5) == 0 &&
std.find(1, [1, 2, 1, 3]) == [0, 2] &&
std.find(5, [1, 2, 3]) == []
); true
