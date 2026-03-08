assert std.zip([1, 2, 3], ["a", "b", "c"]) == [[1, "a"], [2, "b"], [3, "c"]] : "basic zip";
assert std.zip([1, 2], ["a", "b", "c"]) == [[1, "a"], [2, "b"]] : "truncate to shorter";
assert std.zip([], [1, 2]) == [] : "empty first";
assert std.zip([1, 2], []) == [] : "empty second";
true
