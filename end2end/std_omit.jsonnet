assert stdExtended.omit({ a: 1, b: 2, c: 3 }, ["b"]) == { a: 1, c: 3 } : "omit single";
assert stdExtended.omit({ a: 1, b: 2, c: 3 }, ["a", "c"]) == { b: 2 } : "omit multiple";
assert stdExtended.omit({ a: 1 }, ["b"]) == { a: 1 } : "omit absent key is no-op";
assert stdExtended.omit({}, ["a"]) == {} : "empty object";
assert stdExtended.omit({ a: 1, b: 2 }, []) == { a: 1, b: 2 } : "empty keys means keep all";
true
