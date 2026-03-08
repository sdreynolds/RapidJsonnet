assert (
std.flattenDeepArray([[1, 2], [], [3, [4]], [[5, 6, [null]], [7, 8]]]) == [1, 2, 3, 4, 5, 6, null, 7, 8] &&
std.flattenDeepArray([1, 2, 3]) == [1, 2, 3] &&
std.flattenDeepArray([]) == [] &&
std.flattenDeepArray([[["deep"]]]) == ["deep"]
); true
