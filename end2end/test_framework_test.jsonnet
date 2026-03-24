{
  testBasicEquality(): std.assertEqual(1 + 1, 2),
  testStringOps(): std.assertEqual(std.length("hello"), 5),
  testAssertKeyword():
    assert std.type("hello") == "string" : "type check";
    true,
  testArrayLength(): std.assertEqual(std.length([1, 2, 3]), 3),
}
