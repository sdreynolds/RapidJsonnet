assert (
local t = std.type;
local l = std.length;
local a = std.abs;
t(1) == "number" &&
l([1, 2]) == 2 &&
a(-10) == 10
); true
