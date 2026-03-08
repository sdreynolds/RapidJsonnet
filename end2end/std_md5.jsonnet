assert std.md5("") == "d41d8cd98f00b204e9800998ecf8427e" : "empty string";
assert std.md5("hello") == "5d41402abc4b2a76b9719d911017c592" : "hello";
assert std.md5("The quick brown fox jumps over the lazy dog") == "9e107d9d372bb6826bd81d3542a419d6" : "fox";
true
