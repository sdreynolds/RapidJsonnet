// Tail recursion returning a non-number
local f = function(n) if n <= 0 then "done" else f(n - 1) tailstrict;
assert f(1000) == "done" : "deep string result";
true
