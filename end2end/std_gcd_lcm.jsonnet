assert std.gcd(12, 8) == 4 : "gcd(12,8)";
assert std.gcd(7, 3) == 1 : "gcd coprime";
assert std.gcd(0, 5) == 5 : "gcd(0,n)";
assert std.gcd(6, 0) == 6 : "gcd(n,0)";
assert std.lcm(4, 6) == 12 : "lcm(4,6)";
assert std.lcm(7, 3) == 21 : "lcm coprime";
assert std.lcm(12, 8) == 24 : "lcm(12,8)";
assert std.lcm(0, 5) == 0 : "lcm(0,n)";
true
