assert (local arr = [1, 2, 3, 4, 5];

{
  ["key_" + x]: x * x
  for x in arr
  if x % 2 == 0
}) == {"key_2":4.0,"key_4":16.0}; {"key_2":4.0,"key_4":16.0}
