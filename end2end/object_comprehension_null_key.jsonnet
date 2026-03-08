assert (local arr = [1, 2, 3, 4];

{
  [if x % 2 == 0 then "even_" + x else null]: x
  for x in arr
}) == {"even_2":2.0,"even_4":4.0}; {"even_2":2.0,"even_4":4.0}
