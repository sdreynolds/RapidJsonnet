// SHA3-256 produces 64 hex chars
assert std.length(std.sha3("")) == 64 : "sha3 empty length";
assert std.length(std.sha3("hello")) == 64 : "sha3 hello length";
// Known SHA3-256 test vector for empty string
assert std.sha3("") == "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a" : "sha3 empty vector";
true
