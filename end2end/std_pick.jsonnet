assert stdExtended.pick({ a: 1, b: 2, c: 3 }, ["a", "c"]) == { a: 1, c: 3 } : "pick subset";
assert stdExtended.pick({ a: 1, b: 2 }, ["b"]) == { b: 2 } : "pick single";
assert stdExtended.pick({ a: 1 }, ["b"]) == {} : "missing key silently ignored";
assert stdExtended.pick({}, ["a"]) == {} : "empty object";
assert stdExtended.pick({ a: 1, b: 2 }, []) == {} : "empty keys list";
true
