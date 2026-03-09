assert std.manifestXmlJsonml(["p", "Hello"]) == "<p>Hello</p>" : "simple element";
local result = std.manifestXmlJsonml(["div", ["p", "text"]]);
assert result == "<div><p>text</p></div>" : "nested";
local esc = std.manifestXmlJsonml(["p", "<>&"]);
assert std.findSubstr("&lt;", esc) != [] : "lt escaping";
assert std.findSubstr("&gt;", esc) != [] : "gt escaping";
assert std.findSubstr("&amp;", esc) != [] : "amp escaping";

// Mixed text and element children
local p = std.manifestXmlJsonml(["p", "Hello ", ["b", "world"]]);
assert p == "<p>Hello <b>world</b></p>" : "mixed text and element";

// Element with attributes
local with_attrs = std.manifestXmlJsonml(["p", {id: "main"}, "Hello"]);
assert with_attrs == "<p id=\"main\">Hello</p>" : "element with attribute and text";

// Self-closing element with attributes (no children)
local img = std.manifestXmlJsonml(["img", {src: "photo.jpg", alt: "A photo"}]);
assert std.findSubstr("src=\"photo.jpg\"", img) != [] : "src attr";
assert std.findSubstr("alt=\"A photo\"", img) != [] : "alt attr";
true
