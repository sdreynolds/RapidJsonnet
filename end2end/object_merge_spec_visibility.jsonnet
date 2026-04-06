// Spec rule (hidden status inheritance):
// h_L + h_R = h_L if h_R = ':', else h_R
//
// This means:
//   :: + :   = ::   (left hidden, right visible ':' → inherit left's hidden)
//   :  + :   = :    (left visible, right visible → visible)
//   :  + ::  = ::   (right explicitly hides)
//   :  + ::: = :::  (right force-reveals)
//   :: + ::  = ::   (both hidden)
//   :: + ::: = :::  (right force-reveals wins)
//   ::: + :  = :::  (left force-visible, right ':' → inherit left's force-visible)

// Case: :: + : = :: (hidden is inherited when right uses ':')
local obj1 = { x:: 1 } + { x: 2 };
assert std.objectFieldsAll(obj1) == ["x"] : "x should exist in obj1";
assert std.objectFields(obj1) == [] : "x should be hidden in :: + : = ::";
assert obj1.x == 2 : "value should still be accessible directly";

// Case: : + : = : (visible stays visible)
local obj2 = { x: 1 } + { x: 2 };
assert std.objectFields(obj2) == ["x"] : "x should be visible in : + : = :";

// Case: : + :: = :: (right explicitly hides)
local obj3 = { x: 1 } + { x:: 2 };
assert std.objectFields(obj3) == [] : "x should be hidden in : + :: = ::";
assert std.objectFieldsAll(obj3) == ["x"] : "x should still exist hidden";

// Case: ::: + : = ::: (force-visible inherited when right uses ':')
local obj4 = { x::: 1 } + { x: 2 };
assert std.objectFields(obj4) == ["x"] : "x should be visible in ::: + : = :::";

// Case: :: + ::: = ::: (right force-reveals)
local obj5 = { x:: 1 } + { x::: 2 };
assert std.objectFields(obj5) == ["x"] : "x should be visible in :: + ::: = :::";

// Case: hidden field is NOT in objectFields for the merged object
local base = { secret:: "hidden", public: "shown" };
local ext = base + { extra: "new" };
assert std.objectFields(ext) == ["extra", "public"] : "hidden field should not appear in merged fields";

true
