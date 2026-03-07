std.base64("hello") == "aGVsbG8=" &&
std.base64("") == "" &&
std.base64("Man") == "TWFu" &&
std.base64("Ma") == "TWE=" &&
std.base64("M") == "TQ==" &&
std.base64([72, 101, 108, 108, 111]) == "SGVsbG8="
