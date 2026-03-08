assert (std.uniq([1, 1, 2, 3, 3, 3, 4]) == [1, 2, 3, 4] &&
std.uniq(["a", "a", "b", "c", "c"]) == ["a", "b", "c"] &&
std.uniq([]) == [] &&
std.uniq([1, 2, 1, 3]) == [1, 2, 1, 3] &&
std.uniq([{k: 1, v: "a"}, {k: 1, v: "b"}, {k: 2, v: "c"}], function(x) x.k) == [{k: 1, v: "a"}, {k: 2, v: "c"}] &&
std.uniq([1, 2, 3, 4], function(x) x % 2) == [1, 2, 3, 4] &&
std.uniq([], function(x) x) == []); true
