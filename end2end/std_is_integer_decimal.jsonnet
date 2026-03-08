assert (
std.isInteger(1.0) &&
std.isInteger(0.0) &&
std.isInteger(-5.0) &&
!std.isInteger(1.5) &&
!std.isInteger(0.1) &&
std.isDecimal(1.5) &&
std.isDecimal(0.001) &&
!std.isDecimal(2.0) &&
!std.isDecimal(-3.0)
); true
