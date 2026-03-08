local people = [{name: "alice", age: 25}, {name: "bob", age: 30}, {name: "charlie", age: 20}];
assert std.minBy(people, function(p) p.age) == {name: "charlie", age: 20} : "youngest";
assert std.minBy([3, 1, 4, 1, 5], function(x) x) == 1 : "min number";
assert std.minBy(["banana", "apple", "cherry"], function(s) s) == "apple" : "min string";
true
