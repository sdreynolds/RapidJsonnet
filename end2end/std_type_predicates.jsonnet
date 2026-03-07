std.isArray([]) == true &&
std.isArray([1, 2, 3]) == true &&
std.isArray({}) == false &&
std.isArray(null) == false &&
std.isBoolean(true) == true &&
std.isBoolean(false) == true &&
std.isBoolean(0) == false &&
std.isBoolean(null) == false &&
std.isNumber(42) == true &&
std.isNumber(3.14) == true &&
std.isNumber("42") == false &&
std.isNumber(null) == false &&
std.isObject({}) == true &&
std.isObject({a: 1}) == true &&
std.isObject([]) == false &&
std.isObject(null) == false &&
std.isString("hello") == true &&
std.isString("") == true &&
std.isString(42) == false &&
std.isString(null) == false &&
std.isNull(null) == true &&
std.isNull(false) == false &&
std.isNull(0) == false &&
std.isNull("") == false &&
std.isFunction(function(x) x) == true &&
std.isFunction(42) == false
