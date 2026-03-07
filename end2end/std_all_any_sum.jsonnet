std.all([true, true, true]) == true &&
std.all([true, false, true]) == false &&
std.all([]) == true &&
std.any([false, false, true]) == true &&
std.any([false, false, false]) == false &&
std.any([]) == false &&
std.sum([1, 2, 3]) == 6 &&
std.sum([]) == 0
