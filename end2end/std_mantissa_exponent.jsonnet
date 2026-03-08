local check = function(x)
  std.abs(std.mantissa(x) * std.pow(2, std.exponent(x)) - x) < 1e-9;
assert check(1.0) : "1.0";
assert check(0.5) : "0.5";
assert check(100.0) : "100.0";
assert check(-3.14) : "-3.14";
assert std.mantissa(0) == 0 : "mantissa(0)";
assert std.exponent(0) == 0 : "exponent(0)";
assert std.mantissa(1.0) == 0.5 : "mantissa(1.0) == 0.5";
assert std.exponent(1.0) == 1 : "exponent(1.0) == 1";
true
