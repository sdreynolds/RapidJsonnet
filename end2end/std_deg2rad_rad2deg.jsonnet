assert (
local eps = 0.0001;
local near = function(a, b) std.abs(a - b) < eps;

near(std.deg2rad(0), 0) &&
near(std.deg2rad(180), std.pi) &&
near(std.deg2rad(90), std.pi / 2) &&
near(std.rad2deg(0), 0) &&
near(std.rad2deg(std.pi), 180) &&
near(std.rad2deg(std.pi / 2), 90)
); true
