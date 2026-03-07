std.startsWith("foobar", "foo") == true &&
std.startsWith("foobar", "bar") == false &&
std.startsWith("foobar", "") == true &&
std.endsWith("foobar", "bar") == true &&
std.endsWith("foobar", "foo") == false &&
std.endsWith("foobar", "") == true
