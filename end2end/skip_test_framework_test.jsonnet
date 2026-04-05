{
  test_works: function() std.assertEqual(1, 1),
  skip_test_wip: function() std.assertEqual(1, 2),  // This would fail if it wasn't skipped!
  test_also_works: function() std.assertEqual("foo", "foo"),
}
