assert std.type(std.thisFile) == "string";
assert std.length(std.thisFile) > 0;
assert std.thisFile == "test_thisFile.jsonnet";

{
    filename: std.thisFile,
    success: true
}
