local result = std.manifestPythonVars({
  b: ["foo", "bar"],
  c: true,
  d: null,
});
assert std.findSubstr('b = [\n   "foo",\n   "bar"\n]', result) != [] : "array var";
assert std.findSubstr("c = True", result) != [] : "bool var";
assert std.findSubstr("d = None", result) != [] : "null var";
assert std.manifestPythonVars({}) == "" : "empty";
true
