assert (
std.format("Hello %s", "world") == "Hello world" &&
std.format("%d + %d = %d", [1, 2, 3]) == "1 + 2 = 3" &&
std.format("%05d", 42) == "00042" &&
std.format("%.2f", 3.14159) == "3.14" &&
std.format("%(name)s is %(age)d", {name: "Alice", age: 30}) == "Alice is 30" &&
"Hello %s" % "world" == "Hello world" &&
// %% literal percent
std.format("100%%", []) == "100%" &&
// %o zero padding
std.format("%08o", 8) == "00000010" &&
// %x zero padding
std.format("%08x", 255) == "000000ff" &&
// %e two-digit exponent
std.format("%e", 314.159) == "3.141590e+02" &&
// %g switching between f and e
std.format("%g", 0.00001) == "1e-05" &&
std.format("%g", 100000.0) == "100000"
); true
