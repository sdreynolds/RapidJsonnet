// Spec rule (object-inherit): For shared (overridden) fields,
// the right side's expression sees super = the left object.
// e'''_i = local x=self, y=super; e^R_i[e_s/super]
// where e_s = super + left = {} + left = left (at top level access).

// Basic super access in overriding field
local A = { x: 1 };
local B = A + { x: super.x + 1 };
assert B.x == 2 : "super.x in overriding field should be left's x = 1";

// Super access for chained inheritance
local C = { val: 10 };
local D = C + { val: super.val + 5 };
local E = D + { val: super.val + 3 };
assert E.val == 18 : "multi-level super: E.val = D.val+3 = (C.val+5)+3 = 18";

// Super can access multiple fields from left
local F = { a: 1, b: 2 };
local G = F + { c: super.a + super.b };
assert G.c == 3 : "super.a + super.b should reference left's fields";

// Super in overriding field sees the full left chain
local H = { x: 100 };
local I = H + { y: 50 };
local J = I + { z: super.x + super.y };
assert J.z == 150 : "super in J.z sees I which has H as base, so super.x=100, super.y=50";

true
