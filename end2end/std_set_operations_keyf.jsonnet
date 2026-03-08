// setInter with keyF
assert std.setInter(
  [{n:"A", v:1}, {n:"B", v:2}],
  [{n:"A", v:99}, {n:"C", v:3}],
  function(x) x.n
) == [{n:"A", v:1}] : "setInter keyF";

// setDiff with keyF
assert std.setDiff(
  [{n:"A"}, {n:"B"}, {n:"C"}],
  [{n:"B"}],
  function(x) x.n
) == [{n:"A"}, {n:"C"}] : "setDiff keyF";

// setMember with keyF - found
assert std.setMember(
  {n:"B", v:999},
  [{n:"A"}, {n:"B"}, {n:"C"}],
  function(x) x.n
) == true : "setMember found";

// setMember with keyF - not found
assert std.setMember(
  {n:"D"},
  [{n:"A"}, {n:"B"}, {n:"C"}],
  function(x) x.n
) == false : "setMember not found";

// setUnion with keyF
local union = std.setUnion(
  [{n:"A", v:1}, {n:"B", v:2}],
  [{n:"A", v:99}, {n:"C", v:3}],
  function(x) x.n
);
assert std.length(union) == 3 : "setUnion keyF length";
assert union[0].n == "A" : "setUnion keyF first";
assert union[1].n == "B" : "setUnion keyF second";
assert union[2].n == "C" : "setUnion keyF third";

true
