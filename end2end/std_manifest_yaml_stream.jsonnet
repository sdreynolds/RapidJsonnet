// Basic stream
local s1 = std.manifestYamlStream(["a", 1], false, true, true);
assert std.startsWith(s1, "---\n") : "starts with ---";
assert std.endsWith(s1, "...\n") : "ends with ...\n when c_document_end=true";
// Without terminator
local s2 = std.manifestYamlStream(["a"], false, false, true);
assert !std.endsWith(s2, "...\n") : "no trailing ...";
// Empty stream
assert std.manifestYamlStream([], false, false, true) == "" : "empty array yields empty string";
true
