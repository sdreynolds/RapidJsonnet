local people = [{name: "charlie", age: 30}, {name: "alice", age: 25}, {name: "bob", age: 35}];
assert stdExtended.sortBy(people, function(p) p.name)
  == [{name: "alice", age: 25}, {name: "bob", age: 35}, {name: "charlie", age: 30}]
  : "sort by name";
assert stdExtended.sortBy(people, function(p) p.age)
  == [{name: "alice", age: 25}, {name: "charlie", age: 30}, {name: "bob", age: 35}]
  : "sort by age";
assert stdExtended.sortBy([], function(x) x) == [] : "empty array";
assert stdExtended.sortBy([3, 1, 2], function(x) x) == [1, 2, 3] : "sort numbers";
true
