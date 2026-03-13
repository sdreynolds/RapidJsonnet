// Tail-recursive countdown
local f = function(n) if n <= 0 then 0 else f(n - 1) tailstrict;
assert f(0) == 0 : "base case";
assert f(1) == 0 : "one step";
assert f(100) == 0 : "100 steps";
assert f(10000) == 0 : "deep recursion";
true
