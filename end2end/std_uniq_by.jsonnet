local items = [{id: 1, v: "a"}, {id: 2, v: "b"}, {id: 1, v: "c"}];
assert stdExtended.uniqBy(items, function(x) std.toString(x.id))
  == [{id: 1, v: "a"}, {id: 2, v: "b"}] : "dedup by id, keep first";
assert stdExtended.uniqBy([1, 2, 1, 3, 2], function(x) std.toString(x))
  == [1, 2, 3] : "dedup numbers";
assert stdExtended.uniqBy([], function(x) std.toString(x)) == [] : "empty";
true
