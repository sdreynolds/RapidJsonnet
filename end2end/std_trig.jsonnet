local pi = 3.141592653589793;
local eps = 0.0001;
local near = function(a, b) std.abs(a - b) < eps;

near(std.sin(0), 0) &&
near(std.sin(pi), 0) &&
near(std.cos(0), 1) &&
near(std.cos(pi), -1) &&
near(std.tan(0), 0) &&
near(std.tan(pi / 4), 1)
