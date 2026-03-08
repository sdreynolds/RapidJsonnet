assert (std.substr("jsonnet", 0, 4) == "json" &&
std.substr("jsonnet", 4, 10) == "net" &&
std.substr("hello", 1, 3) == "ell" &&
std.substr("hello", 2, 0) == "" &&
std.substr("hello", 10, 3) == ""); true
