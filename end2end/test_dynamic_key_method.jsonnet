// Test dynamic key with method shorthand: ["f"](x, y, z):: expr
assert std.assertEqual({ ["f"](x, y, z):: x, "y"(x): self.f(x, 2, 3), z: self.y(4) }.z, 4);

true
