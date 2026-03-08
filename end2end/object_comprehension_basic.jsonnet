assert (local arr = [
  {k: "a", v: 1},
  {k: "b", v: 2},
  {k: "c", v: 3}
];

{
  [x.k]: x.v * 10
  for x in arr
}) == {"a":10.0,"b":20.0,"c":30.0}; {"a":10.0,"b":20.0,"c":30.0}
