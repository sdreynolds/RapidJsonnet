local pi = 3.141592653589793;
local eps = 0.0001;
local near = function(a, b) std.abs(a - b) < eps;

near(std.asin(0), 0) &&
near(std.asin(1), pi / 2) &&
near(std.acos(0), pi / 2) &&
near(std.acos(1), 0) &&
near(std.atan(0), 0) &&
near(std.atan(1), pi / 4) &&
near(std.atan2(1, 1), pi / 4) &&
near(std.atan2(1, 0), pi / 2) &&
near(std.atan2(0, -1), pi)
