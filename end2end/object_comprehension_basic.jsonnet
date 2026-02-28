local arr = [
  {k: "a", v: 1},
  {k: "b", v: 2},
  {k: "c", v: 3}
];

{
  [x.k]: x.v * 10
  for x in arr
}