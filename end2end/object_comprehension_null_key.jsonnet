local arr = [1, 2, 3, 4];

{
  [if x % 2 == 0 then "even_" + x else null]: x
  for x in arr
}