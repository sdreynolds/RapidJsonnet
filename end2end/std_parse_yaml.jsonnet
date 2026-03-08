assert std.parseYaml("foo: bar") == { foo: "bar" } : "simple mapping";
assert std.parseYaml("- 1\n- 2\n- 3") == [1, 2, 3] : "sequence";
assert std.parseYaml("key: 42") == { key: 42 } : "integer value";
assert std.parseYaml("{}") == {} : "empty mapping";
assert std.parseYaml("[]") == [] : "empty sequence";
true
