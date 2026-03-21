// Test the 'in' membership operator

// Basic 'in' operator: check if field exists in object
assert "x" in { x: 1, y: 2 };
assert !("z" in { x: 1, y: 2 });

// 'in' with super
assert ({ x: 1 } + { a: "x" in super, b: "y" in super }) == { "x": 1, "a": true, "b": false };

// 'in' with hidden fields (in checks all fields including hidden)
assert "h" in { h:: 1 };

// 'in' used in if-then-else with dynamic key +: override
assert ({ opt:: true, f: { y: 5 } } + { f+: { [if "opt" in super then "x" else "y"]+: 3 } })
    == { "f": { "x": 3, "y": 5 } };

true
