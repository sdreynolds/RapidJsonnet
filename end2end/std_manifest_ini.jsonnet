local ini = {
  main: { a: "1", b: "2" },
  sections: {
    s1: { x: "11", y: "22" },
    s2: { p: "yes" },
  },
};
local result = std.manifestIni(ini);
assert std.startsWith(result, "a = 1\n") : "main section keys come first";
assert std.findSubstr("[s1]", result) != [] : "s1 section present";
assert std.findSubstr("[s2]", result) != [] : "s2 section present";
assert std.findSubstr("x = 11", result) != [] : "x = 11 present";
assert std.findSubstr("p = yes", result) != [] : "p = yes present";
assert std.manifestIni({ main: {}, sections: {} }) == "" : "empty yields empty string";

// Empty section
local with_empty = std.manifestIni({
  main: {},
  sections: { empty: {} },
});
assert std.findSubstr("[empty]", with_empty) != [] : "empty section header present";
true
