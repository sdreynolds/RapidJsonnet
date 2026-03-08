assert (std.parseInt("123") == 123 &&
std.parseInt("-42") == -42 &&
std.parseOctal("755") == 493 &&
std.parseOctal("0755") == 493 &&
std.parseHex("ff") == 255 &&
std.parseHex("FF") == 255 &&
std.parseHex("DeAdBeEf") == 3735928559); true
