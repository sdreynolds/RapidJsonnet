assert (std.base64DecodeBytes("aGVsbG8=") == [104, 101, 108, 108, 111] &&
std.base64DecodeBytes("") == [] &&
std.base64DecodeBytes("TWFu") == [77, 97, 110]); true
