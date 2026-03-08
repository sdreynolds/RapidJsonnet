local people = [{name: "alice", age: 25}, {name: "bob", age: 30}, {name: "charlie", age: 20}];
assert std.maxBy(people, function(p) p.age) == {name: "bob", age: 30} : "oldest";
assert std.maxBy([3, 1, 4, 1, 5], function(x) x) == 5 : "max number";
assert std.maxBy(["banana", "apple", "cherry"], function(s) s) == "cherry" : "max string";
true
