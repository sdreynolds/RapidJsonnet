assert std.manifestXmlJsonml(["p", "Hello"]) == "<p>Hello</p>" : "simple element";
local result = std.manifestXmlJsonml(["div", ["p", "text"]]);
assert result == "<div><p>text</p></div>" : "nested";
local esc = std.manifestXmlJsonml(["p", "<>&"]);
assert std.findSubstr("&lt;", esc) != [] : "lt escaping";
assert std.findSubstr("&gt;", esc) != [] : "gt escaping";
assert std.findSubstr("&amp;", esc) != [] : "amp escaping";
true
