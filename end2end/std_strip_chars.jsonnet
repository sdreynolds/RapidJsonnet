std.stripChars(" test test test ", " ") == "test test test" &&
std.stripChars("aaabbbbcccc", "ac") == "bbbb" &&
std.stripChars("cacabbbbaacc", "ac") == "bbbb" &&
std.lstripChars(" test ", " ") == "test " &&
std.rstripChars(" test ", " ") == " test" &&
std.trim("  hello world  ") == "hello world" &&
std.trim("\t\nhello\n") == "hello" &&
std.trim("\t hello \n") == "hello"
