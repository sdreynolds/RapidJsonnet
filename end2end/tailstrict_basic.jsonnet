// Basic sanity: tailstrict on a non-recursive call
local g = function(x) x + 1;
local f1 = function() g(5) tailstrict;
local f2 = function() g(41) tailstrict;
assert f1() == 6 : "basic tailstrict";
assert f2() == 42 : "basic tailstrict 2";
true
