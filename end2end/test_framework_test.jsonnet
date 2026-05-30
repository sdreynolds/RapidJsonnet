local root = import "end2end/import_integration_test.libsonnet";

{
  testBasicEquality(): std.assertEqual(root.rootValue + root.rootValue, 2),
    testStringOps(): std.assertEqual(std.length(root.stringValue()), 5),
  testAssertKeyword():
    assert std.type(root.stringValue()) == "string" : "type check";
    true,
  testArrayLength(): std.assertEqual(std.length(root.arrayValue), 3),
  testBranch(): std.assertEqual(root.branchTest(true), "true"),
  testBranchFalse(): std.assertEqual(root.branchTest(false), "false"),
}
