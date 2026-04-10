// Test: object field thunks must be cached after first evaluation.
//
// This builds a chain of 250 objects where each .x forces the parent's .x thunk.
// Then accesses the deepest .x field 20000 times via tailstrict recursion.
//
// Without caching: 250 * 20000 = 5,000,000 thunk re-evaluations → very slow
// With caching:    250 + 19999 = ~20,249 evaluations → instant
local chain(n) =
  if n == 0 then { x: 0 }
  else local prev = chain(n - 1); { x: prev.x + 1 };

local deep = chain(250);

local repeat(n, acc) =
  if n == 0 then acc
  else repeat(n - 1, acc + deep.x) tailstrict;

repeat(20000, 0)
