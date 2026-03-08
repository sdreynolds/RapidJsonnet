assert (std.repeat([1, 2, 3], 3) == [1, 2, 3, 1, 2, 3, 1, 2, 3] &&
std.repeat("blah", 2) == "blahblah" &&
std.repeat([], 5) == [] &&
std.repeat("x", 0) == "" &&
std.repeat([1, 2], 0) == []); true
