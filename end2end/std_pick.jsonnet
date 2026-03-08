assert std.pick({ a: 1, b: 2, c: 3 }, ["a", "c"]) == { a: 1, c: 3 } : "pick subset";
assert std.pick({ a: 1, b: 2 }, ["b"]) == { b: 2 } : "pick single";
assert std.pick({ a: 1 }, ["b"]) == {} : "missing key silently ignored";
assert std.pick({}, ["a"]) == {} : "empty object";
assert std.pick({ a: 1, b: 2 }, []) == {} : "empty keys list";
true
