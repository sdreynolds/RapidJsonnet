local obj = {a: 1, b: 2, c: 3};
local objWithHidden = {a:: 1, b: 2};
local objSortCheck = {z: 1, a: 2};
std.objectFields(obj) == ["a", "b", "c"] &&
std.objectHas(obj, "a") == true &&
std.objectHas(obj, "z") == false &&
std.objectFields(objWithHidden) == ["b"] &&
std.objectHas(objWithHidden, "a") == false &&
std.objectHas(objWithHidden, "b") == true &&
std.objectFields(objSortCheck) == ["a", "z"]
