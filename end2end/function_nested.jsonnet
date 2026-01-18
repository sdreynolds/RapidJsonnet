local outer = function(x) { local inner = function(y) { x + y }; inner(10) }; outer(5)
