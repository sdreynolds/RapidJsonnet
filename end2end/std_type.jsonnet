std.type(null) == "null" &&
std.type(true) == "boolean" &&
std.type(1) == "number" &&
std.type("a") == "string" &&
std.type({}) == "object" &&
std.type([]) == "array" &&
std.type(function(x) x) == "function"
