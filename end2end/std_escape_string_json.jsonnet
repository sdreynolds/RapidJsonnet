std.escapeStringJson("hello") == "\"hello\"" &&
std.escapeStringJson("say \"hi\"") == "\"say \\\"hi\\\"\"" &&
std.escapeStringJson("line1\nline2") == "\"line1\\nline2\""
