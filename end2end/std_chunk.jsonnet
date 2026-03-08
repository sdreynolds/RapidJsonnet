assert std.chunk([1, 2, 3, 4, 5], 2) == [[1, 2], [3, 4], [5]] : "basic chunking";
assert std.chunk([1, 2, 3, 4], 2) == [[1, 2], [3, 4]] : "even split";
assert std.chunk([], 3) == [] : "empty array";
assert std.chunk([1], 5) == [[1]] : "chunk larger than array";
assert std.chunk([1, 2, 3], 1) == [[1], [2], [3]] : "chunk of 1";
true
