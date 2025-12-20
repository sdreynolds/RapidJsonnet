local makeAdder = function(x) function(y) x + y;
local add5 = makeAdder(5);
add5(3)
