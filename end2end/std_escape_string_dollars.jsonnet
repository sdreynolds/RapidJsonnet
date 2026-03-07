std.escapeStringDollars("hello") == "hello" &&
std.escapeStringDollars("$var") == "$$var" &&
std.escapeStringDollars("${VAR}") == "$${VAR}" &&
std.escapeStringDollars("a$$b") == "a$$$$b"
