assert (
local obj = { a: 1, b: 2, c: 3 };
local result = std.objectRemoveKey(obj, "b");

std.objectFields(result) == ["a", "c"] &&
result.a == 1 &&
result.c == 3 &&
std.objectFields(std.objectRemoveKey(obj, "z")) == ["a", "b", "c"] &&
std.objectFields(std.objectRemoveKey({}, "x")) == [] &&

// super inside object that is itself merged: remove key from the merged object.
// Field b's thunk has super = {a:1}, so b evaluates to 1.
std.objectRemoveKey({ a: 1 } + { b: super.a }, "a") == { b: 1 } &&

// super inside standalone object passed to objectRemoveKey, then merged outside.
// The + after the call provides the super context.
{ a: 1 } + std.objectRemoveKey({ b: super.a }, "a") == { a: 1, b: 1 } &&

// Referential transparency: binding the object to a local first changes nothing.
(local o1 = { b: super.a }; std.objectRemoveKey({ a: 1 } + o1, "a")) == { b: 1 } &&
(local o1 = { b: super.a }; { a: 1 } + std.objectRemoveKey(o1, "a")) == { a: 1, b: 1 } &&

// Hidden field (::) is preserved and accessible via direct access.
std.objectFields(std.objectRemoveKey({ a: 1 } + { b:: super.a }, "a")) == [] &&
std.objectRemoveKey({ a: 1 } + { b:: super.a }, "a").b == 1 &&
std.objectFields({ a: 1 } + std.objectRemoveKey({ b:: super.a }, "a")) == ["a"] &&
({ a: 1 } + std.objectRemoveKey({ b:: super.a }, "a")).b == 1
); true
