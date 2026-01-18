local mkAdder = function(x) {
  local adder = function(y) { x + y };
  adder
};
local add5 = mkAdder(5);
add5(10)
