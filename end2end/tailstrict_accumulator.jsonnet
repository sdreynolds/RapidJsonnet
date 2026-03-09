// Accumulator pattern - classic TCO use case
local sum = function(n, acc)
  if n <= 0 then acc
  else tailstrict sum(n - 1, acc + n);
assert sum(0, 0) == 0 : "empty sum";
assert sum(10, 0) == 55 : "sum 1..10";
assert sum(100, 0) == 5050 : "sum 1..100";
true
