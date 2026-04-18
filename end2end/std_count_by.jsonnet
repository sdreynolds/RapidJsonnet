assert stdExtended.countBy(["a", "b", "a", "c", "b", "a"], function(x) x)
  == {a: 3, b: 2, c: 1} : "count chars";
assert stdExtended.countBy([1, 2, 3, 4, 5, 6], function(x) if x % 2 == 0 then "even" else "odd")
  == {odd: 3, even: 3} : "count even/odd";
assert stdExtended.countBy([], function(x) x) == {} : "empty array";
true
