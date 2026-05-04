local y = {some_object: "yep"};
local x = function(n) (
  n + y
);

local upperFunction = function() (
  x(2)
);

local firstFunction = function() (
  upperFunction() + 2
);

{
  should_fail: firstFunction()
}
