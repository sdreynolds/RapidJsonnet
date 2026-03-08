assert std.indent("line1\nline2\nline3", "  ")
  == "  line1\n  line2\n  line3" : "indent 3 lines";
assert std.indent("hello", ">>") == ">>hello" : "single line";
assert std.indent("", "  ") == "" : "empty string";
assert std.indent("a\nb\n", "- ") == "- a\n- b\n" : "trailing newline preserved";
true
