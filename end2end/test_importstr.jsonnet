local data = importstr "test_importstr_data.txt";
{
    data: data,
    equal: data == "Hello, this is a test for importstr!
It has multiple lines.
12345
Bye.
"
}
