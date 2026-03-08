assert (
std.join(".", ["www", "google", "com"]) == "www.google.com" &&
std.join([9, 9], [[1], [2, 3]]) == [1, 9, 9, 2, 3] &&
std.join(".", []) == "" &&
std.join([9, 9], []) == []
); true
