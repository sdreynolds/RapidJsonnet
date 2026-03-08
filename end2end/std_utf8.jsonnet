assert (std.encodeUTF8("ABC") == [65, 66, 67] &&
std.decodeUTF8([65, 66, 67]) == "ABC" &&
std.decodeUTF8(std.encodeUTF8("hello")) == "hello" &&
std.encodeUTF8("€") == [226, 130, 172] &&
std.decodeUTF8([226, 130, 172]) == "€" &&
std.decodeUTF8(std.encodeUTF8("日本語")) == "日本語"); true
