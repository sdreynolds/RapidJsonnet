std.setInter([1, 2, 3], [2, 3, 4]) == [2, 3] &&
std.setInter([1, 2, 3], [4, 5, 6]) == [] &&
std.setInter([1, 2, 3], [1, 2, 3]) == [1, 2, 3] &&
std.setInter([], [1, 2]) == [] &&
std.setInter([1, 2], []) == []
