std.filterMap(function(x) x > 2, function(x) x * 10, [1, 2, 3, 4, 5]) == [30, 40, 50] &&
std.filterMap(function(x) false, function(x) x, [1, 2, 3]) == [] &&
std.filterMap(function(x) true, function(x) x + '!', ['a', 'b']) == ['a!', 'b!']
