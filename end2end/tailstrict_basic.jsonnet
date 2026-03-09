// Basic sanity: tailstrict on a non-recursive call
local g = function(x) x + 1;
local f1 = function() tailstrict g(5);
local f2 = function() tailstrict g(41);
assert f1() == 6 : "basic tailstrict";
assert f2() == 42 : "basic tailstrict 2";
true
