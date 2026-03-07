std.parseJson('{"foo": "bar"}') == {foo: "bar"} &&
std.parseJson('[1, 2, 3]') == [1, 2, 3] &&
std.parseJson('"hello"') == "hello" &&
std.parseJson('null') == null &&
std.parseJson('true') == true &&
std.parseJson('42') == 42
