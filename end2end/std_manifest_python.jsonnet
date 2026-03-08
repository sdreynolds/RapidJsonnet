assert std.manifestPython(null) == "None" : "null is None";
assert std.manifestPython(true) == "True" : "true is True";
assert std.manifestPython(false) == "False" : "false is False";
assert std.manifestPython(42) == "42" : "integer";
assert std.manifestPython("hello") == '"hello"' : "string";
assert std.manifestPython([1, true, null]) == "[\n   1,\n   True,\n   None\n]" : "array";
true
