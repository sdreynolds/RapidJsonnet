local result = std.manifestPythonVars({
  b: ["foo", "bar"],
  c: true,
  d: null,
});
assert std.findSubstr('b = [\n   "foo",\n   "bar"\n]', result) != [] : "array var";
assert std.findSubstr("c = True", result) != [] : "bool var";
assert std.findSubstr("d = None", result) != [] : "null var";
assert std.manifestPythonVars({}) == "" : "empty";

local nested = std.manifestPythonVars({e: {f1: false, f2: 42}});
assert std.findSubstr("e =", nested) != [] : "var e";
assert std.findSubstr("False", nested) != [] : "False in nested";
true
