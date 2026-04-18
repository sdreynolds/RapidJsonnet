assert stdExtended.unzip([[1, "a"], [2, "b"], [3, "c"]]) == [[1, 2, 3], ["a", "b", "c"]] : "basic unzip";
assert stdExtended.unzip([]) == [[], []] : "empty yields two empty arrays";
assert stdExtended.unzip([[true, null]]) == [[true], [null]] : "single pair";
true
