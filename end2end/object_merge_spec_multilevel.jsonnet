// Multi-level inheritance: A + B + C
// Each level's super should point to the accumulated left chain.

// Three-level chain with super references
local A = { x: 1 };
local B = A + { x: super.x + 1 };  // B.x = 2
local C = B + { x: super.x + 1 };  // C.x = 3
assert C.x == 3 : "C.x should be 3 (super chain: C→B→A)";

// Self threading through multiple levels
local Base = { x: 1, y: self.x * 2 };
local Mid = Base + { x: 5 };       // Mid.y should see self.x = 5
local Top = Mid + { z: self.y };   // Top.z = Top.y = self.x*2 = 5*2 = 10
assert Top.z == 10 : "Top.z = Top.y = self.x*2 = 10";

// Left-only fields propagate through chain with correct self
local P = { name: "base", full: self.name + "_v1" };
local Q = P + { name: "ext" };           // Q.full should see self.name = "ext"
local R = Q + { extra: self.full };      // R.extra = R.full = R.name + "_v1"
assert R.extra == "ext_v1" : "R.extra should be 'ext_v1'";

// Deeply nested super chain
local D1 = { v: 1 };
local D2 = D1 + { v: super.v + 10 };
local D3 = D2 + { v: super.v + 100 };
local D4 = D3 + { v: super.v + 1000 };
assert D4.v == 1111 : "D4.v = 1 + 10 + 100 + 1000 = 1111";

true
