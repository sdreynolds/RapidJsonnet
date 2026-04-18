// Regression tests for lazy object inheritance: verify that std functions
// correctly walk the full base_object chain introduced by commit 2a5d469.
//
// Pattern: { a:1, b:2 } + { c:3, d:4 } produces a chained object where
// the top node holds {c,d} and the base holds {a,b}.  Functions that only
// inspect the top node see 2 fields instead of 4.
//
// Candidates under test:
//   std.mapWithKey, stdExtended.toPairs, stdExtended.mapKeys, stdExtended.filterObject,
//   stdExtended.objectFlatten, is_truthy (if/then on object), % string format

local merged = { a: 1, b: 2 } + { c: 3, d: 4 };

// ── std.mapWithKey ────────────────────────────────────────────────────────────
local mwk = std.mapWithKey(function(k, v) v * 10, merged);
assert std.length(mwk) == 4
  : "mapWithKey: expected 4 fields, got " + std.length(mwk);
assert std.objectHas(mwk, "a") : "mapWithKey: missing inherited key a";
assert std.objectHas(mwk, "b") : "mapWithKey: missing inherited key b";
assert mwk.a == 10 : "mapWithKey: wrong value for a, got " + mwk.a;
assert mwk.b == 20 : "mapWithKey: wrong value for b, got " + mwk.b;
assert mwk.c == 30 : "mapWithKey: wrong value for c, got " + mwk.c;

// ── stdExtended.toPairs ───────────────────────────────────────────────────────────────
local pairs = stdExtended.toPairs(merged);
assert std.length(pairs) == 4
  : "toPairs: expected 4 pairs, got " + std.length(pairs);
// toPairs returns [[key, value], ...] arrays — access key with p[0]
local pairKeys = std.map(function(p) p[0], pairs);
assert std.member(pairKeys, "a") : "toPairs: missing key a";
assert std.member(pairKeys, "b") : "toPairs: missing key b";

// ── stdExtended.mapKeys ───────────────────────────────────────────────────────────────
local mk = stdExtended.mapKeys(function(k) k + "x", merged);
assert std.length(std.objectFields(mk)) == 4
  : "mapKeys: expected 4 fields, got " + std.length(std.objectFields(mk));
assert std.objectHas(mk, "ax") : "mapKeys: missing ax (inherited a)";
assert std.objectHas(mk, "bx") : "mapKeys: missing bx (inherited b)";

// ── stdExtended.filterObject ─────────────────────────────────────────────────────────
// func signature: function(key, value) -> bool
local fo_all = stdExtended.filterObject(function(k, v) true, merged);
assert std.length(fo_all) == 4
  : "filterObject(true): expected 4, got " + std.length(fo_all);

local fo_gt1 = stdExtended.filterObject(function(k, v) v > 1, merged);
// a=1 excluded, b=2, c=3, d=4 kept → 3 fields
assert std.length(fo_gt1) == 3
  : "filterObject(v>1): expected 3, got " + std.length(fo_gt1);
assert !std.objectHas(fo_gt1, "a") : "filterObject: a should be filtered out";
assert std.objectHas(fo_gt1, "b") : "filterObject: b should be kept";

// ── stdExtended.objectFlatten ────────────────────────────────────────────────────────
// nested merged object; both branches should be flattened
local nested = { x: { p: 1, q: 2 } } + { y: { r: 3 } };
local flat = stdExtended.objectFlatten(nested, ".");
local flatFields = std.objectFields(flat);
assert std.length(flatFields) == 3
  : "objectFlatten: expected 3 flat fields, got " + std.length(flatFields);
assert std.member(flatFields, "x.p") : "objectFlatten: missing x.p (inherited)";
assert std.member(flatFields, "x.q") : "objectFlatten: missing x.q (inherited)";
assert std.member(flatFields, "y.r") : "objectFlatten: missing y.r (top)";

// ── is_truthy: merged object where only the base has fields ───────────────────
// { a:1 } + {} → top node is {}, base is {a:1}
// is_truthy checks load_object(top).len() which is 0 → falsy (bug)
local left_has_fields = { a: 1 } + {};
assert (if left_has_fields then true else false)
  : "is_truthy: {a:1}+{} should be truthy but was falsy";

// ── % string format with inherited fields ─────────────────────────────────────
// force_object_fields only walks top-level node, so format cannot see a/b
local fmt_obj = { a: "hello", b: "world" } + { c: "!" };
local fmt_result = "%(a)s %(b)s %(c)s" % fmt_obj;
assert fmt_result == "hello world !"
  : "% format: expected 'hello world !', got '" + fmt_result + "'";

"PASS"
