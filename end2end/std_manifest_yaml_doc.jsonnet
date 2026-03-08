// Basic types (with explicit default args: indent_array_in_object=false, quote_keys=true)
assert std.manifestYamlDoc(null, false, true) == "null" : "null";
assert std.manifestYamlDoc(true, false, true) == "true" : "true";
assert std.manifestYamlDoc(false, false, true) == "false" : "false";
assert std.manifestYamlDoc(42, false, true) == "42" : "int";
assert std.manifestYamlDoc("hello", false, true) == "hello" : "unquoted string";
// Empty collections
assert std.manifestYamlDoc({}, false, true) == "{ }" : "empty object";
assert std.manifestYamlDoc([], false, true) == "[ ]" : "empty array";
// Quoted keys by default
local result = std.manifestYamlDoc({ a: 1, b: "hello" }, false, true);
assert std.findSubstr('"a":', result) != [] : "quoted key a";
// No-quote-keys
local result2 = std.manifestYamlDoc({ x: true }, false, false);
assert std.findSubstr("x: true", result2) != [] : "unquoted key x";
true
