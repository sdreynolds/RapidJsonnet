local t = std.manifestTomlEx({
  key1: "value",
  key2: 1,
  section: { a: 1, b: "str" },
}, "  ");
assert std.findSubstr('key1 = "value"', t) != [] : "key1 string";
assert std.findSubstr("key2 = 1", t) != [] : "key2 int";
assert std.findSubstr("[section]", t) != [] : "section header";
// Array of tables
local t2 = std.manifestTomlEx({
  items: [{ k: "v1" }, { k: "v2" }],
}, "");
assert std.findSubstr("[[items]]", t2) != [] : "array of tables";
assert std.findSubstr('k = "v1"', t2) != [] : "first item";
true
