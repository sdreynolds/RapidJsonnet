local eps = 0.0001;
local near = function(a, b) std.abs(a - b) < eps;

near(std.log2(1), 0) &&
near(std.log2(2), 1) &&
near(std.log2(8), 3) &&
near(std.log10(1), 0) &&
near(std.log10(10), 1) &&
near(std.log10(1000), 3)
