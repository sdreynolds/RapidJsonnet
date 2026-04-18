assert stdExtended.indent("line1\nline2\nline3", "  ")
  == "  line1\n  line2\n  line3" : "indent 3 lines";
assert stdExtended.indent("hello", ">>") == ">>hello" : "single line";
assert stdExtended.indent("", "  ") == "" : "empty string";
assert stdExtended.indent("a\nb\n", "- ") == "- a\n- b\n" : "trailing newline preserved";
true
