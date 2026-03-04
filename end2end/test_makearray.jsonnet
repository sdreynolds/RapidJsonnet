local arr = std.makeArray(5, function(i) i * 2);
assert arr == [0, 2, 4, 6, 8];
assert std.length(std.makeArray(0, function(i) i)) == 0;
true