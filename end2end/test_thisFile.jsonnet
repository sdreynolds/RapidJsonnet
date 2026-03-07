assert std.type(std.thisFile) == "string";
assert std.length(std.thisFile) > 0;
assert std.thisFile == "end2end/test_thisFile.jsonnet";

{
    filename: std.thisFile,
    success: true
}
