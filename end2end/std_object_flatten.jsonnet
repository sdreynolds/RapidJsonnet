assert std.objectFlatten({ a: { b: 1, c: { d: 2 } }, e: 3 }, ".")
  == { "a.b": 1, "a.c.d": 2, e: 3 } : "nested flatten";
assert std.objectFlatten({}, ".") == {} : "empty";
assert std.objectFlatten({ x: 1, y: 2 }, "/") == { x: 1, y: 2 } : "already flat";
assert std.objectFlatten({ a: { b: [1, 2, 3] } }, ".") == { "a.b": [1, 2, 3] } : "array as leaf";
true
