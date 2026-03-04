assert std.codepoint("A") == 65;
assert std.char(65) == "A";
// Check unicode logic
assert std.codepoint("😊") == 128522;
assert std.char(128522) == "😊";
true