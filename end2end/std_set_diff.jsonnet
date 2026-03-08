assert (std.setDiff([1, 2, 3, 4], [2, 4]) == [1, 3] &&
std.setDiff([1, 2, 3], [4, 5]) == [1, 2, 3] &&
std.setDiff([1, 2, 3], [1, 2, 3]) == [] &&
std.setDiff([], [1, 2]) == [] &&
std.setDiff([1, 2], []) == [1, 2]); true
