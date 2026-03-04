# Jsonnet - Standard Library
This page describes the functions available in Jsonnet's standard library, i.e. the object implicitly bound to the `std` variable. Some of the standard library functions can be implemented in Jsonnet. Their code can be found in the std.jsonnet file. The behavior of some of the other functions, i.e. the ones that expose extra functionality not otherwise available to programmers, is described formally in the [specification](https://jsonnet.org/language/spec.html).

The standard library is implicitly added to all Jsonnet programs by enclosing them in a local construct. For example, if the program given by the user is `{x: "foo"}`, then the actual code executed would be `local std = { ... }; {x: "foo"}`. The functions in the standard library are all hidden fields of the `std` object.

Note: Some of these functions marked available since v0.10.0 were actually available earlier.

### External Variables

#### std.extVar(x)

_Available since version 0.10.0._

If an external variable with the given name was defined, return its value. Otherwise, raise an error.

### Types and Reflection

#### std.thisFile

_Available since version 0.10.0._

Note that this is a field. It contains the current Jsonnet filename as a string.

#### std.type(x)

_Available since version 0.10.0._

Return a string that indicates the type of the value. The possible return values are: "array", "boolean", "function", "null", "number", "object", and "string".

The following functions are also available and return a boolean: `std.isArray(v)`, `std.isBoolean(v)`, `std.isFunction(v)`, `std.isNumber(v)`, `std.isObject(v)`, and `std.isString(v)`.

#### std.length(x)

_Available since version 0.10.0._

Depending on the type of the value given, either returns the number of elements in the array, the number of codepoints in the string, the number of parameters in the function, or the number of fields in the object. Raises an error if given a primitive value, i.e. `null`, `true` or `false`.

#### std.prune(a)

_Available since version 0.10.0._

Recursively remove all "empty" members of `a`. "Empty" is defined as zero length \`arrays\`, zero length \`objects\`, or \`null\` values. The argument `a` may have any type.

#### std.isNull(x)

_Available in upcoming release._

Returns true if the given value is null, false otherwise.

Example: `std.isNull(null)` yields `true`.

Example: `std.isNull(42)` yields `false`.

Example: `std.isNull("string")` yields `false`.

### Mathematical Utilities

The following mathematical functions are available:

`std.abs(n)`

`std.sign(n)`

`std.max(a, b)`

`std.min(a, b)`

`std.pow(x, n)`

`std.exp(x)`

`std.log(x)`

`std.log2(x)` (available since 0.21.0)

`std.log10(x)` (available since 0.21.0)

`std.exponent(x)`

`std.mantissa(x)`

`std.floor(x)`

`std.ceil(x)`

`std.sqrt(x)`

`std.sin(x)`

`std.cos(x)`

`std.tan(x)`

`std.asin(x)`

`std.acos(x)`

`std.atan(x)`

`std.atan2(y, x)` (available since 0.21.0)

`std.deg2rad(x)` (available since 0.21.0)

`std.rad2deg(x)` (available since 0.21.0)

`std.hypot(a, b)` (available since 0.21.0)

`std.round(x)` (available since 0.20.0)

`std.isEven(x)` (available since 0.21.0)

`std.isOdd(x)` (available since 0.21.0)

`std.isInteger(x)` (available since 0.21.0)

`std.isDecimal(x)` (available since 0.21.0)

The constant `std.pi` is also available (since 0.21.0).

The function `std.mod(a, b)` is what the % operator is desugared to. It performs modulo arithmetic if the left hand side is a number, or if the left hand side is a string, it does Python-style string formatting with `std.format()`.

The functions `std.isEven(x)` and `std.isOdd(x)` use integral part of a floating number to test for even or odd.

#### std.clamp(x, minVal, maxVal)

_Available since version 0.15.0._

Clamp a value to fit within the range \[`minVal`, `maxVal`\]. Equivalent to `std.max(minVal, std.min(x, maxVal))`.

Example: `std.clamp(-3, 0, 5)` yields `0`.

Example: `std.clamp(4, 0, 5)` yields `4`.

Example: `std.clamp(7, 0, 5)` yields `5`.

### Assertions and Debugging

#### std.assertEqual(a, b)

_Available since version 0.10.0._

Ensure that `a == b`. Returns `true` or throws an error message.

### String Manipulation

#### std.toString(a)

_Available since version 0.10.0._

Convert the given argument to a string.

#### std.codepoint(str)

_Available since version 0.10.0._

Returns the positive integer representing the unicode codepoint of the character in the given single-character string. This function is the inverse of `std.char(n)`.

#### std.char(n)

_Available since version 0.10.0._

Returns a string of length one whose only unicode codepoint has integer id `n`. This function is the inverse of `std.codepoint(str)`.

#### std.substr(str, from, len)

_Available since version 0.10.0._

Returns a string that is the part of `str` that starts at offset `from` and is `len` codepoints long. If the string `str` is shorter than `from+len`, the suffix starting at position `from` will be returned.

The slice operator (e.g., `str[from:to]`) can also be used on strings, as an alternative to this function. However, note that the slice operator takes a start and an end index, but `std.substr` takes a start index and a length.

#### std.findSubstr(pat, str)

_Available since version 0.10.0._

Returns an array that contains the indexes of all occurrences of `pat` in `str`.

#### std.startsWith(a, b)

_Available since version 0.10.0._

Returns whether the string a is prefixed by the string b.

#### std.endsWith(a, b)

_Available since version 0.10.0._

Returns whether the string a is suffixed by the string b.

#### std.stripChars(str, chars)

_Available since version 0.15.0._

Removes characters `chars` from the beginning and from the end of `str`.

Example: `std.stripChars(" test test test ", " ")` yields `"test test test"`.

Example: `std.stripChars("aaabbbbcccc", "ac")` yields `"bbbb"`.

Example: `std.stripChars("cacabbbbaacc", "ac")` yields `"bbbb"`.

#### std.lstripChars(str, chars)

_Available since version 0.15.0._

Removes characters `chars` from the beginning of `str`.

Example: `std.lstripChars(" test test test ", " ")` yields `"test test test "`.

Example: `std.lstripChars("aaabbbbcccc", "ac")` yields `"bbbbcccc"`.

Example: `std.lstripChars("cacabbbbaacc", "ac")` yields `"bbbbaacc"`.

#### std.rstripChars(str, chars)

_Available since version 0.15.0._

Removes characters `chars` from the end of `str`.

Example: `std.rstripChars(" test test test ", " ")` yields `" test test test"`.

Example: `std.rstripChars("aaabbbbcccc", "ac")` yields `"aaabbbb"`.

Example: `std.rstripChars("cacabbbbaacc", "ac")` yields `"cacabbbb"`.

#### std.split(str, c)

_Available since version 0.10.0._

Split the string `str` into an array of strings, divided by the string `c`.

Note: Versions up to and including 0.18.0 require `c` to be a single character.

Example: `std.split("foo/_bar", "/_")` yields `[ "foo", "bar" ]`.

Example: `std.split("/_foo/_bar", "/_")` yields `[ "", "foo", "bar" ]`.

#### std.splitLimit(str, c, maxsplits)

_Available since version 0.10.0._

As `std.split(str, c)` but will stop after `maxsplits` splits, thereby the largest array it will return has length `maxsplits + 1`. A limit of `-1` means unlimited.

Note: Versions up to and including 0.18.0 require `c` to be a single character.

Example: `std.splitLimit("foo/_bar", "/_", 1)` yields `[ "foo", "bar" ]`.

Example: `std.splitLimit("/_foo/_bar", "/_", 1)` yields `[ "", "foo/_bar" ]`.

#### std.splitLimitR(str, c, maxsplits)

_Available since version 0.19.0._

As `std.splitLimit(str, c, maxsplits)` but will split from right to left.

Example: `std.splitLimitR("/_foo/_bar", "/_", 1)` yields `[ "/_foo", "bar" ]`.

#### std.strReplace(str, from, to)

_Available since version 0.10.0._

Returns a copy of the string in which all occurrences of string `from` have been replaced with string `to`.

Example: `std.strReplace('I like to skate with my skateboard', 'skate', 'surf')` yields `"I like to surf with my surfboard"`.

#### std.isEmpty(str)

_Available since version 0.20.0._

Returns true if the given string is of zero length.

#### std.trim(str)

_Available since version 0.21.0._

Returns a copy of string after eliminating leading and trailing whitespaces.

#### std.equalsIgnoreCase(str1, str2)

_Available since version 0.21.0._

Returns true if the the given `str1` is equal to `str2` by doing case insensitive comparison, false otherwise.

#### std.asciiUpper(str)

_Available since version 0.10.0._

Returns a copy of the string in which all ASCII letters are capitalized.

Example: `std.asciiUpper('100 Cats!')` yields `"100 CATS!"`.

#### std.asciiLower(str)

_Available since version 0.10.0._

Returns a copy of the string in which all ASCII letters are lower cased.

Example: `std.asciiLower('100 Cats!')` yields `"100 cats!"`.

#### std.stringChars(str)

_Available since version 0.10.0._

Split the string `str` into an array of strings, each containing a single codepoint.

Example: `std.stringChars("foo")` yields `[ "f", "o", "o" ]`.

#### std.format(str, vals)

_Available since version 0.10.0._

Format the string `str` using the values in `vals`. The values can be an array, an object, or in other cases are treated as if they were provided in a singleton array. The string formatting follows the [same rules](https://docs.python.org/2/library/stdtypes.html#string-formatting) as Python. The `%` operator can be used as a shorthand for this function.

Example: `std.format("Hello %03d", 12)` yields `"Hello 012"`.

Example: `"Hello %03d" % 12` yields `"Hello 012"`.

Example: `"Hello %s, age %d" % ["Foo", 25]` yields `"Hello Foo, age 25"`.

Example: `"Hello %(name)s, age %(age)d" % {age: 25, name: "Foo"}` yields `"Hello Foo, age 25"`.

#### std.escapeStringBash(str)

_Available since version 0.10.0._

Wrap `str` in single quotes, and escape any single quotes within `str` by changing them to a sequence '"'"'. This allows injection of arbitrary strings as arguments of commands in bash scripts.

#### std.escapeStringDollars(str)

_Available since version 0.10.0._

Convert $ to $$ in `str`. This allows injection of arbitrary strings into systems that use $ for string interpolation (like Terraform).

#### std.escapeStringJson(str)

_Available since version 0.10.0._

Convert `str` to allow it to be embedded in a JSON representation, within a string. This adds quotes, escapes backslashes, and escapes unprintable characters.

Example: `local description = "Multiline\nc:\\path"; "{name: %s}" % std.escapeStringJson(description)` yields `"{name: \"Multiline\\nc:\\\\path\"}"`.

#### std.escapeStringPython(str)

_Available since version 0.10.0._

Convert `str` to allow it to be embedded in Python. This is an alias for `std.escapeStringJson`.

#### std.escapeStringXml(str)

_Available since version 0.10.0._

Convert `str` to allow it to be embedded in XML (or HTML). The following replacements are made:

```
        {
          "<": "&lt;",
          ">": "&gt;",
          "&": "&amp;",
          "\"": "&quot;",
          "'": "&apos;",
        }

```


### Parsing

#### std.parseInt(str)

_Available since version 0.10.0._

Parses a signed decimal integer from the input string.

Example: `std.parseInt("123")` yields `123`.

Example: `std.parseInt("-123")` yields `-123`.

#### std.parseOctal(str)

_Available since version 0.10.0._

Parses an unsigned octal integer from the input string. Initial zeroes are tolerated.

Example: `std.parseOctal("755")` yields `493`.

#### std.parseHex(str)

_Available since version 0.10.0._

Parses an unsigned hexadecimal integer, from the input string. Case insensitive.

Example: `std.parseHex("ff")` yields `255`.

#### std.parseJson(str)

_Available since version 0.13.0._

Parses a JSON string.

Example: `std.parseJson('{"foo": "bar"}')` yields `{ "foo": "bar" }`.

#### std.parseYaml(str)

_Available since version 0.18.0._

Parses a YAML string. This is provided as a "best-effort" mechanism and should not be relied on to provide a fully standards compliant YAML parser. YAML is a superset of JSON, consequently "downcasting" or manifestation of YAML into JSON or Jsonnet values will only succeed when using the subset of YAML that is compatible with JSON. The parser does not support YAML documents with scalar values at the root. The root node of a YAML document must start with either a YAML sequence or map to be successfully parsed.

Example: `std.parseYaml('foo: bar')` yields `{ "foo": "bar" }`.

#### std.encodeUTF8(str)

_Available since version 0.13.0._

Encode a string using [UTF8](https://en.wikipedia.org/wiki/UTF-8). Returns an array of numbers representing bytes.

#### std.decodeUTF8(arr)

_Available since version 0.13.0._

Decode an array of numbers representing bytes using [UTF8](https://en.wikipedia.org/wiki/UTF-8). Returns a string.

### Manifestation

#### std.manifestIni(ini)

_Available since version 0.10.0._

Convert the given structure to a string in [INI format](https://en.wikipedia.org/wiki/INI_file). This allows using Jsonnet's object model to build a configuration to be consumed by an application expecting an INI file. The data is in the form of a set of sections, each containing a key/value mapping. These examples should make it clear:

```
{
    main: { a: "1", b: "2" },
    sections: {
        s1: {x: "11", y: "22", z: "33"},
        s2: {p: "yes", q: ""},
        empty: {},
    }
}
```


Yields a string containing this INI file:

```
a = 1
b = 2
[empty]
[s1]
x = 11
y = 22
z = 33
[s2]
p = yes
q =
```


#### std.manifestPython(v)

_Available since version 0.10.0._

Convert the given value to a JSON-like form that is compatible with Python. The chief differences are True / False / None instead of true / false / null.

```
{
    b: ["foo", "bar"],
    c: true,
    d: null,
    e: { f1: false, f2: 42 },
}
```


Yields a string containing Python code like:

```
{
    "b": ["foo", "bar"],
    "c": True,
    "d": None,
    "e": {"f1": False, "f2": 42}
}
```


#### std.manifestPythonVars(conf)

_Available since version 0.10.0._

Convert the given object to a JSON-like form that is compatible with Python. The key difference to `std.manifestPython` is that the top level is represented as a list of Python global variables.

```
{
    b: ["foo", "bar"],
    c: true,
    d: null,
    e: { f1: false, f2: 42 },
}
```


Yields a string containing this Python code:

```
b = ["foo", "bar"]
c = True
d = None
e = {"f1": False, "f2": 42}
```


#### std.manifestJsonEx(value, indent, newline, key\_val\_sep)

_Available since version 0.10.0._

Convert the given object to a JSON form. `indent` is a string containing one or more whitespaces that are used for indentation. `newline` is by default `\n` and is inserted where a newline would normally be used to break long lines. `key_val_sep` is used to separate the key and value of an object field:

Example:

```
std.manifestJsonEx(
{
    x: [1, 2, 3, true, false, null,
        "string\nstring"],
    y: { a: 1, b: 2, c: [1, 2] },
}, "    ")
```


Yields a string containing this JSON:

```
{
    "x": [
        1,
        2,
        3,
        true,
        false,
        null,
        "string\nstring"
    ],
    "y": {
        "a": 1,
        "b": 2,
        "c": [
            1,
            2
        ]
    }
}
```


Example:

```
std.manifestJsonEx(
{
  x: [1, 2, "string\nstring"],
  y: { a: 1, b: [1, 2] },
}, "", " ", " : ")
```


Yields a string containing this JSON:

```
{ "x" : [ 1, 2, "string\nstring" ], "y" : { "a" : 1, "b" : [ 1, 2 ] } }
```


#### std.manifestJson(value)

_Available since version 0.10.0._

Convert the given object to a JSON form. Under the covers, it calls `std.manifestJsonEx` with a 4-space indent:

Example:

```
std.manifestJson(
{
    x: [1, 2, 3, true, false, null,
        "string\nstring"],
    y: { a: 1, b: 2, c: [1, 2] },
})
```


Yields a string containing this JSON:

```
{
    "x": [
        1,
        2,
        3,
        true,
        false,
        null,
        "string\nstring"
    ],
    "y": {
        "a": 1,
        "b": 2,
        "c": [
            1,
            2
        ]
    }
}
```


#### std.manifestJsonMinified(value)

_Available since version 0.18.0._

Convert the given object to a minified JSON form. Under the covers, it calls `std.manifestJsonEx`:

Example:

```
std.manifestJsonMinified(
{
    x: [1, 2, 3, true, false, null,
        "string\nstring"],
    y: { a: 1, b: 2, c: [1, 2] },
})
```


Yields a string containing this JSON:

```
{"x":[1,2,3,true,false,null,"string\nstring"],"y":{"a":1,"b":2,"c":[1,2]}}
```


#### std.manifestYamlDoc(value, indent\_array\_in\_object=false, quote\_keys=true)

_Available since version 0.10.0._

Convert the given value to a YAML form. Note that `std.manifestJson` could also be used for this purpose, because any JSON is also valid YAML. But this function will produce more canonical-looking YAML.

```
std.manifestYamlDoc(
  {
      x: [1, 2, 3, true, false, null,
          "string\nstring\n"],
      y: { a: 1, b: 2, c: [1, 2] },
  },
  indent_array_in_object=false)
```


Yields a string containing this YAML:

```
"x":
  - 1
  - 2
  - 3
  - true
  - false
  - null
  - |
      string
      string
"y":
  "a": 1
  "b": 2
  "c":
      - 1
      - 2
```


The `indent_array_in_object` param adds additional indentation which some people may find easier to read.

The `quote_keys` parameter controls whether YAML identifiers are always quoted or only when necessary.

#### std.manifestYamlStream(value, indent\_array\_in\_object=false, c\_document\_end=false, quote\_keys=true)

_Available since version 0.10.0._

Given an array of values, emit a YAML "stream", which is a sequence of documents separated by `---` and ending with `...`.

```
std.manifestYamlStream(
  ['a', 1, []],
  indent_array_in_object=false,
  c_document_end=true)
```


Yields this string:

```
---
"a"
---
1
---
[]
...
```


The `indent_array_in_object` and `quote_keys` params are the same as in `manifestYamlDoc`.

The `c_document_end` param adds the optional terminating `...`.

#### std.manifestXmlJsonml(value)

_Available since version 0.10.0._

Convert the given [JsonML](http://www.jsonml.org/)\-encoded value to a string containing the XML.

```
std.manifestXmlJsonml([
    'svg', { height: 100, width: 100 },
    [
        'circle', {
        cx: 50, cy: 50, r: 40,
        stroke: 'black', 'stroke-width': 3,
        fill: 'red',
        }
    ],
])
```


Yields a string containing this XML (all on one line):

```
<svg height="100" width="100">
    <circle cx="50" cy="50" fill="red" r="40"
    stroke="black" stroke-width="3"></circle>;
</svg>;
```


Which represents the following image:

Sorry, your browser does not support inline SVG.

JsonML is designed to preserve "mixed-mode content" (i.e., textual data outside of or next to elements). This includes the whitespace needed to avoid having all the XML on one line, which is meaningful in XML. In order to have whitespace in the XML output, it must be present in the JsonML input:

```
std.manifestXmlJsonml([
    'svg',
    { height: 100, width: 100 },
    '\n  ',
    [
        'circle',
        {
        cx: 50, cy: 50, r: 40, stroke: 'black',
        'stroke-width': 3, fill: 'red',
        }
    ],
    '\n',
])
```


#### std.manifestTomlEx(value, indent)

_Available since version 0.18.0._

Convert the given object to a TOML form. `indent` is a string containing one or more whitespaces that are used for indentation:

Example:

```
std.manifestTomlEx({
  key1: "value",
  key2: 1,
  section: {
    a: 1,
    b: "str",
    c: false,
    d: [1, "s", [2, 3]],
    subsection: {
      k: "v",
    },
  },
  sectionArray: [
    { k: "v1", v: 123 },
    { k: "v2", c: "value2" },
  ],
}, "  ")
```


Yields a string containing this TOML file:

```
key1 = "value"
key2 = 1

[section]
  a = 1
  b = "str"
  c = false
  d = [
    1,
    "s",
    [ 2, 3 ]
  ]

  [section.subsection]
    k = "v"

[[sectionArray]]
  k = "v1"
  v = 123

[[sectionArray]]
  c = "value2"
  k = "v2"
```


### Arrays

#### std.makeArray(sz, func)

_Available since version 0.10.0._

Create a new array of `sz` elements by calling `func(i)` to initialize each element. Func is expected to be a function that takes a single parameter, the index of the element it should initialize.

Example: `std.makeArray(3,function(x) x * x)` yields `[ 0, 1, 4 ]`.

#### std.member(arr, x)

_Available since version 0.15.0._

Returns whether `x` occurs in `arr`. Argument `arr` may be an array or a string.

#### std.count(arr, x)

_Available since version 0.10.0._

Return the number of times that `x` occurs in `arr`.

#### std.find(value, arr)

_Available since version 0.10.0._

Returns an array that contains the indexes of all occurrences of `value` in `arr`.

#### std.map(func, arr)

_Available since version 0.10.0._

Apply the given function to every element of the array to form a new array.

#### std.mapWithIndex(func, arr)

_Available since version 0.10.0._

Similar to [map](#map) above, but it also passes to the function the element's index in the array. The function `func` is expected to take the index as the first parameter and the element as the second.

#### std.filterMap(filter\_func, map\_func, arr)

_Available since version 0.10.0._

It first filters, then maps the given array, using the two functions provided.

#### std.flatMap(func, arr)

_Available since version 0.10.0._

Apply the given function to every element of `arr` to form a new array then flatten the result. The argument `arr` must be an array or a string. If `arr` is an array, function `func` must return an array. If `arr` is a string, function `func` must return an string.

The `std.flatMap` function can be thought of as a generalized `std.map`, with each element mapped to 0, 1 or more elements.

Example: `std.flatMap(function(x) [x, x], [1, 2, 3])` yields `[ 1, 1, 2, 2, 3, 3 ]`.

Example: `std.flatMap(function(x) if x == 2 then [] else [x], [1, 2, 3])` yields `[ 1, 3 ]`.

Example: `std.flatMap(function(x) if x == 2 then [] else [x * 3, x * 2], [1, 2, 3])` yields `[ 3, 2, 9, 6 ]`.

Example: `std.flatMap(function(x) x+x, "foo")` yields `"ffoooo"`.

#### std.filter(func, arr)

_Available since version 0.10.0._

Return a new array containing all the elements of `arr` for which the `func` function returns true.

#### std.foldl(func, arr, init)

_Available since version 0.10.0._

Classic foldl function. Calls the function for each array element, passing the result from the previous call (or `init` for the first call), and the array element. Traverses the array from left to right.

For example: `foldl(f, [1,2,3], 0)` is equivalent to `f(f(f(0, 1), 2), 3)`.

#### std.foldr(func, arr, init)

_Available since version 0.10.0._

Classic foldr function. Calls the function for each array element, passing the array element and the result from the previous call (or `init` for the first call). Traverses the array from right to left.

For example: `foldr(f, [1,2,3], 0)` is equivalent to `f(1, f(2, f(3, 0)))`.

#### std.range(from, to)

_Available since version 0.10.0._

Return an array of ascending numbers between the two limits, inclusively.

#### std.repeat(what, count)

_Available since version 0.15.0._

Repeats an array or a string `what` a number of times specified by an integer `count`.

Example: `std.repeat([1, 2, 3], 3)` yields `[ 1, 2, 3, 1, 2, 3, 1, 2, 3 ]`.

Example: `std.repeat("blah", 2)` yields `"blahblah"`.

#### std.slice(indexable, index, end, step)

_Available since version 0.10.0._

Selects the elements of an array or a string from `index` to `end` with `step` and returns an array or a string respectively.

Note that it's recommended to use dedicated slicing syntax both for arrays and strings (e.g. `arr[0:4:1]` instead of `std.slice(arr, 0, 4, 1)`).

Example: `std.slice([1, 2, 3, 4, 5, 6], 0, 4, 1)` yields `[ 1, 2, 3, 4 ]`.

Example: `std.slice([1, 2, 3, 4, 5, 6], 1, 6, 2)` yields `[ 2, 4, 6 ]`.

Example: `std.slice("jsonnet", 0, 4, 1)` yields `"json"`.

Example: `std.slice("jsonnet", -3, null, null)` yields `"net"`.

#### std.join(sep, arr)

_Available since version 0.10.0._

If `sep` is a string, then `arr` must be an array of strings, in which case they are concatenated with `sep` used as a delimiter. If `sep` is an array, then `arr` must be an array of arrays, in which case the arrays are concatenated in the same way, to produce a single array.

Example: `std.join(".", ["www", "google", "com"])` yields `"www.google.com"`.

Example: `std.join([9, 9], [[1], [2, 3]])` yields `[ 1, 9, 9, 2, 3 ]`.

#### std.deepJoin(arr)

_Available since version 0.10.0._

Concatenate an array containing strings and arrays to form a single string. If `arr` is a string, it is returned unchanged. If it is an array, it is flattened and the string elements are concatenated together with no separator.

Example: `std.deepJoin(["one ", ["two ", "three ", ["four "], []], "five ", ["six"]])` yields `"one two three four five six"`.

Example: `std.deepJoin("hello")` yields `"hello"`.

#### std.lines(arr)

_Available since version 0.10.0._

Concatenate an array of strings into a text file with newline characters after each string. This is suitable for constructing bash scripts and the like.

#### std.flattenArrays(arr)

_Available since version 0.10.0._

Concatenate an array of arrays into a single array.

Example: `std.flattenArrays([[1, 2], [3, 4], [[5, 6], [7, 8]]])` yields `[ 1, 2, 3, 4, [ 5, 6 ], [ 7, 8 ] ]`.

#### std.flattenDeepArray(value)

_Available since version 0.21.0._

Concatenate an array containing values and arrays into a single flattened array.

Example: `std.flattenDeepArray([[1, 2], [], [3, [4]], [[5, 6, [null]], [7, 8]]])` yields `[ 1, 2, 3, 4, 5, 6, null, 7, 8 ]`.

#### std.reverse(arrs)

_Available since version 0.13.0._

Reverses an array.

#### std.sort(arr, keyF=id)

_Available since version 0.10.0._

Sorts the array using the <= operator.

Optional argument `keyF` is a single argument function used to extract comparison key from each array element. Default value is identity function `keyF=function(x) x`.

#### std.uniq(arr, keyF=id)

_Available since version 0.10.0._

Removes successive duplicates. When given a sorted array, removes all duplicates.

Optional argument `keyF` is a single argument function used to extract comparison key from each array element. Default value is identity function `keyF=function(x) x`.

#### std.all(arr)

_Available since version 0.19.0._

Return true if all elements of `arr` is true, false otherwise. `all([])` evaluates to true.

It's an error if 1) `arr` is not an array, or 2) `arr` contains non-boolean values.

#### std.any(arr)

_Available since version 0.19.0._

Return true if any element of `arr` is true, false otherwise. `any([])` evaluates to false.

It's an error if 1) `arr` is not an array, or 2) `arr` contains non-boolean values.

#### std.sum(arr)

_Available since version 0.20.0._

Return sum of all element in `arr`.

#### std.minArray(arr, keyF, onEmpty)

_Available since version 0.21.0._

Return the minimum of all elements in `arr`. If `keyF` is provided, it is called on each element of the array and should return a comparator value, and in this case `minArray` will return an element with the minimum comparator value. If `onEmpty` is provided, and `arr` is empty, then `minArray` will return the provided `onEmpty` value. If `onEmpty` is not provided, then an empty `arr` will raise an error.

#### std.maxArray(arr, keyF, onEmpty)

_Available since version 0.21.0._

Return the maximum of all elements in `arr`. If `keyF` is provided, it is called on each element of the array and should return a comparator value, and in this case `maxArray` will return an element with the maximum comparator value. If `onEmpty` is provided, and `arr` is empty, then `maxArray` will return the provided `onEmpty` value. If `onEmpty` is not provided, then an empty `arr` will raise an error.

#### std.contains(arr, elem)

_Available since version 0.21.0._

Return true if given `elem` is present in `arr`, false otherwise.

#### std.avg(arr)

_Available since version 0.21.0._

Return average of all element in `arr`.

#### std.remove(arr, elem)

_Available since version 0.21.0._

Remove first occurrence of `elem` from `arr`.

#### std.removeAt(arr, idx)

_Available since version 0.21.0._

Remove element at `idx` index from `arr`.

### Sets

Sets are represented as ordered arrays without duplicates.

Note that the `std.set*` functions rely on the uniqueness and ordering on arrays passed to them to work. This can be guaranteed by using `std.set(arr)`. If that is not the case, the functions will quietly return non-meaningful results.

All `set.set*` functions accept `keyF` function of one argument, which can be used to extract key to use from each element. All Set operations then use extracted key for the purpose of identifying uniqueness. Default value is identity function `local id = function(x) x`.

#### std.set(arr, keyF=id)

_Available since version 0.10.0._

Shortcut for std.uniq(std.sort(arr)).

#### std.setInter(a, b, keyF=id)

_Available since version 0.10.0._

Set intersection operation (values in both a and b).

#### std.setUnion(a, b, keyF=id)

_Available since version 0.10.0._

Set union operation (values in any of `a` or `b`). Note that + on sets will simply concatenate the arrays, possibly forming an array that is not a set (due to not being ordered without duplicates).

Example: `std.setUnion([1, 2], [2, 3])` yields `[ 1, 2, 3 ]`.

Example: `std.setUnion([{n:"A", v:1}, {n:"B"}], [{n:"A", v: 9999}, {n:"C"}], keyF=function(x) x.n)` yields `[ { "n": "A", "v": 1 }, { "n": "B" }, { "n": "C" } ]`.

#### std.setDiff(a, b, keyF=id)

_Available since version 0.10.0._

Set difference operation (values in a but not b).

#### std.setMember(x, arr, keyF=id)

_Available since version 0.10.0._

Returns `true` if x is a member of array, otherwise `false`.

### Objects

#### std.get(o, f, default=null, inc\_hidden=true)

_Available since version 0.18.0._

Returns the object's field if it exists or default value otherwise. `inc_hidden` controls whether to include hidden fields.

#### std.objectHas(o, f)

_Available since version 0.10.0._

Returns `true` if the given object has the field (given as a string), otherwise `false`. Raises an error if the arguments are not object and string respectively. Returns false if the field is hidden.

#### std.objectFields(o)

_Available since version 0.10.0._

Returns an array of strings, each element being a field from the given object. Does not include hidden fields.

#### std.objectValues(o)

_Available since version 0.17.0._

Returns an array of the values in the given object. Does not include hidden fields.

#### std.objectKeysValues(o)

_Available since version 0.20.0._

Returns an array of objects from the given object, each object having two fields: `key` (string) and `value` (object). Does not include hidden fields.

#### std.objectHasAll(o, f)

_Available since version 0.10.0._

As `std.objectHas` but also includes hidden fields.

#### std.objectFieldsAll(o)

_Available since version 0.10.0._

As `std.objectFields` but also includes hidden fields.

#### std.objectValuesAll(o)

_Available since version 0.17.0._

As `std.objectValues` but also includes hidden fields.

#### std.objectKeysValuesAll(o)

_Available since version 0.20.0._

As `std.objectKeysValues` but also includes hidden fields.

#### std.objectRemoveKey(obj, key)

_Available since version 0.21.0._

Returns a new object after removing the given key from object.

#### std.mapWithKey(func, obj)

_Available since version 0.10.0._

Apply the given function to all fields of the given object, also passing the field name. The function `func` is expected to take the field name as the first parameter and the field value as the second.

### Encoding

#### std.base64(input)

_Available since version 0.10.0._

Encodes the given value into a base64 string. The encoding sequence is `A-Za-z0-9+/` with `=` to pad the output to a multiple of 4 characters. The value can be a string or an array of numbers, but the codepoints / numbers must be in the 0 to 255 range. The resulting string has no line breaks.

#### std.base64DecodeBytes(str)

_Available since version 0.10.0._

Decodes the given base64 string into an array of bytes (number values). Currently assumes the input string has no linebreaks and is padded to a multiple of 4 (with the = character). In other words, it consumes the output of std.base64().

#### std.base64Decode(str)

_Available since version 0.10.0._

_Deprecated, use `std.base64DecodeBytes` and decode the string explicitly (e.g. with `std.decodeUTF8`) instead._

Behaves like std.base64DecodeBytes() except returns a naively encoded string instead of an array of bytes.

#### std.md5(s)

_Available since version 0.10.0._

Encodes the given value into an MD5 string.

#### std.sha1(s)

_Available since version 0.21.0._

Encodes the given value into an SHA1 string.

This function is only available in Go version of jsonnet.

#### std.sha256(s)

_Available since version 0.21.0._

Encodes the given value into an SHA256 string.

This function is only available in Go version of jsonnet.

#### std.sha512(s)

_Available since version 0.21.0._

Encodes the given value into an SHA512 string.

This function is only available in Go version of jsonnet.

#### std.sha3(s)

_Available since version 0.21.0._

Encodes the given value into an SHA3 string.

This function is only available in Go version of jsonnet.

### Booleans

#### std.xor(x, y)

_Available since version 0.20.0._

Returns the xor of the two given booleans.

#### std.xnor(x, y)

_Available since version 0.20.0._

Returns the xnor of the two given booleans.

### JSON Merge Patch

#### std.mergePatch(target, patch)

_Available since version 0.10.0._

Applies `patch` to `target` according to [RFC7396](https://tools.ietf.org/html/rfc7396)

### Debugging

#### std.trace(str, rest)

_Available since version 0.11.0._

Outputs the given string `str` to stderr and returns `rest` as the result.

Example:

```
local conditionalReturn(cond, in1, in2) =
  if (cond) then
      std.trace('cond is true returning '
              + std.toString(in1), in1)
  else
      std.trace('cond is false returning '
              + std.toString(in2), in2);

{
    a: conditionalReturn(true, { b: true }, { c: false }),
}
```


Prints:

```
TRACE: test.jsonnet:3 cond is true returning {"b": true}
{
    "a": {
        "b": true
    }
}
```


Except as noted, this content is licensed under Creative Commons Attribution 2.5.
