assert std.groupBy([1, 2, 3, 4, 5, 6], function(x) if x % 2 == 0 then "even" else "odd")
  == { even: [2, 4, 6], odd: [1, 3, 5] } : "group even/odd";
assert std.groupBy(["foo", "bar", "baz", "qux"], function(x) std.substr(x, 0, 1))
  == { b: ["bar", "baz"], f: ["foo"], q: ["qux"] } : "group by first char";
assert std.groupBy([], function(x) x) == {} : "empty array";
assert std.groupBy([1, 2, 3], function(x) "all") == { all: [1, 2, 3] } : "single group";
true
