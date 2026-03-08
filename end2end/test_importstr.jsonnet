assert (local data = importstr "test_importstr_data.txt";
{
    data: data,
    equal: data == "Hello, this is a test for importstr!\nIt has multiple lines.\n12345\nBye.\n"
}) == {"data":"Hello, this is a test for importstr!\nIt has multiple lines.\n12345\nBye.\n","equal":true}; {"data":"Hello, this is a test for importstr!\nIt has multiple lines.\n12345\nBye.\n","equal":true}