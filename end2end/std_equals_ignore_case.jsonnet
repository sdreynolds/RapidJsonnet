assert (
std.equalsIgnoreCase("hello", "HELLO") == true &&
std.equalsIgnoreCase("Hello", "hello") == true &&
std.equalsIgnoreCase("abc", "xyz") == false &&
std.equalsIgnoreCase("", "") == true
); true
