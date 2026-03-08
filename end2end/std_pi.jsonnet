assert (local eps = 0.0001;
local near = function(a, b) std.abs(a - b) < eps;

std.pi > 3.14159 &&
std.pi < 3.14160 &&
near(std.sin(std.pi / 2), 1) &&
near(std.deg2rad(180), std.pi)); true
