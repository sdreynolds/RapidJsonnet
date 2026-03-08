assert (local A = {
  x: 1,
  y: $.x + 1
};
local B = A + {
  x: 10
};
B.y) == 11.0; 11.0
