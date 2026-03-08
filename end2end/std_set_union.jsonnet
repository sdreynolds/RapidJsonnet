std.setUnion([1, 2], [2, 3]) == [1, 2, 3] &&
std.setUnion([1, 2, 3], [1, 2, 3]) == [1, 2, 3] &&
std.setUnion([], [1, 2]) == [1, 2] &&
std.setUnion([1, 2], []) == [1, 2] &&
std.setUnion([], []) == []
