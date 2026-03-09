// Tail recursion returning a non-number
local f = function(n) if n <= 0 then "done" else tailstrict f(n - 1);
assert f(1000) == "done" : "deep string result";
true
