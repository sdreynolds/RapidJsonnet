assert stdExtended.gcd(12, 8) == 4 : "gcd(12,8)";
assert stdExtended.gcd(7, 3) == 1 : "gcd coprime";
assert stdExtended.gcd(0, 5) == 5 : "gcd(0,n)";
assert stdExtended.gcd(6, 0) == 6 : "gcd(n,0)";
assert stdExtended.lcm(4, 6) == 12 : "lcm(4,6)";
assert stdExtended.lcm(7, 3) == 21 : "lcm coprime";
assert stdExtended.lcm(12, 8) == 24 : "lcm(12,8)";
assert stdExtended.lcm(0, 5) == 0 : "lcm(0,n)";
true
