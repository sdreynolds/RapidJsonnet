local g = function(x) x + 1;
local f1 = function() tailstrict g(5) + 1;
f1()
