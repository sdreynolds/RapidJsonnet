std.mergePatch({a: 1, b: 2}, {b: 3, c: 4}) == {a: 1, b: 3, c: 4} &&
std.mergePatch({a: 1, b: 2}, {b: null}) == {a: 1} &&
std.mergePatch({a: {x: 1, y: 2}}, {a: {y: 3}}) == {a: {x: 1, y: 3}} &&
std.mergePatch(42, {a: 1}) == {a: 1}
