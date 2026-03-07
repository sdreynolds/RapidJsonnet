std.deepJoin("hello") == "hello" &&
std.deepJoin(["a", ["b", "c"], "d"]) == "abcd" &&
std.deepJoin(["a", ["b", ["c", "d"]], "e"]) == "abcde" &&
std.deepJoin([]) == ""
