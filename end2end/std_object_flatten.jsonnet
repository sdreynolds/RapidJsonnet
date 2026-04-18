assert stdExtended.objectFlatten({ a: { b: 1, c: { d: 2 } }, e: 3 }, ".")
  == { "a.b": 1, "a.c.d": 2, e: 3 } : "nested flatten";
assert stdExtended.objectFlatten({}, ".") == {} : "empty";
assert stdExtended.objectFlatten({ x: 1, y: 2 }, "/") == { x: 1, y: 2 } : "already flat";
assert stdExtended.objectFlatten({ a: { b: [1, 2, 3] } }, ".") == { "a.b": [1, 2, 3] } : "array as leaf";
true
