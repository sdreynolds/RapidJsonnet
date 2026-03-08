// SHA512 produces 128 hex chars
assert std.length(std.sha512("")) == 128 : "sha512 empty length";
assert std.length(std.sha512("hello")) == 128 : "sha512 hello length";
// Known SHA-512 test vector for empty string
assert std.sha512("") == "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e" : "sha512 empty vector";
true
