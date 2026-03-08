assert std.product([[1, 2], [3, 4]]) == [[1, 3], [1, 4], [2, 3], [2, 4]] : "2x2";
assert std.product([]) == [[]] : "empty input yields one empty tuple";
assert std.product([[1, 2], []]) == [] : "empty sub-array yields empty";
assert std.product([["a", "b"]]) == [["a"], ["b"]] : "single array";
true
