assert (std.sort([3, 1, 4, 1, 5, 9, 2, 6]) == [1, 1, 2, 3, 4, 5, 6, 9] &&
std.sort(["banana", "apple", "cherry"]) == ["apple", "banana", "cherry"] &&
std.sort([]) == [] &&
std.sort([{k: 3}, {k: 1}, {k: 2}], function(x) x.k) == [{k: 1}, {k: 2}, {k: 3}] &&
std.sort(["banana", "apple", "cherry"], function(x) std.length(x)) == ["apple", "banana", "cherry"] &&
std.sort([3, 1, 2], function(x) -x) == [3, 2, 1] &&
std.sort([], function(x) x) == []); true
