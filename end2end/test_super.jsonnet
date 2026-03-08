assert (local A = {
  x: 1,
  y: self.x + 1
};
local B = A + {
  x: 10,
  z: super.y + 1
};
B.z) == 12.0; 12.0
