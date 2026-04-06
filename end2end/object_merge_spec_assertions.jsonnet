// Spec rule: merged object includes assertions from both left and right.
// Assertions run with self = merged object.

// Both assertions should pass (left assert self.x > 0, right assert self.y > 0)
local obj1 = { x: 5, assert self.x > 0 } + { y: 3, assert self.y > 0 };
assert obj1.y == 3 : "accessing obj1.y should trigger both assertions and succeed";
assert obj1.x == 5 : "obj1.x should be 5";

// Left assertion runs with self = merged object (sees overridden field)
local base2 = { x: 1, assert self.x > 0 };
local merged2 = base2 + { x: 100 };
assert merged2.x == 100 : "left assertion should pass since merged x = 100 > 0";

// Right assertion also runs with self = merged object
local base3 = { a: 10 };
local merged3 = base3 + { b: 20, assert self.a + self.b == 30 };
assert merged3.b == 20 : "assertion checking self.a + self.b == 30 should pass";

true
