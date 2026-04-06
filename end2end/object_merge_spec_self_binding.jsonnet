// Spec rule: When accessing any field of a merged object,
// self refers to the ROOT merged object, not the left or right.
// object-inherit rule: self = o (the final merged object).

// Left-only field sees self = merged object
local A = { x: 1, y: self.x + 10 };
local B = A + { x: 5 };
assert B.y == 15 : "left-only field y should see self.x = 5 (from B, not A)";

// Right-only field also sees self = merged object
local C = { x: 3 };
local D = C + { z: self.x * 2 };
assert D.z == 6 : "right-only field z should see self.x = 3";

// Shared field sees self = merged object
local E = { x: 1, w: self.x + 100 };
local F = E + { x: 7, w: self.x * 2 };
assert F.w == 14 : "overridden field w should see self.x = 7";

true
