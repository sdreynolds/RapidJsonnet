std.escapeStringPython("hello") == "\"hello\"" &&
std.escapeStringPython("a\nb") == "\"a\\nb\"" &&
std.escapeStringPython("say \"hi\"") == std.escapeStringJson("say \"hi\"")
