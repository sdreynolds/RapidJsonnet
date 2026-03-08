assert (
std.min(1, 2) == 1 &&
std.min(2, 1) == 1 &&
std.min(-5, 3) == -5 &&
std.min(4.0, 4.0) == 4.0 &&
std.max(1, 2) == 2 &&
std.max(2, 1) == 2 &&
std.max(-5, 3) == 3 &&
std.max(4.0, 4.0) == 4.0 &&
std.sign(5) == 1 &&
std.sign(-5) == -1 &&
std.sign(0) == 0 &&
std.sign(3.14) == 1 &&
std.sign(-3.14) == -1
); true
