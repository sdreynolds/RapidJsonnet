assert std.sha1("") == "da39a3ee5e6b4b0d3255bfef95601890afd80709" : "empty string";
assert std.sha1("hello") == "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d" : "hello";
assert std.length(std.sha1("test")) == 40 : "40 hex chars";
true
