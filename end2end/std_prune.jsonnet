assert (std.prune(null) == null &&
std.prune([1, null, 2]) == [1, 2] &&
std.prune({a: 1, b: null}) == {a: 1} &&
std.prune({a: [], b: [1]}) == {b: [1]} &&
std.prune({x: {y: null}}) == {}); true
