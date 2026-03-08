assert (// end2end/importbin_test.jsonnet
local bytes = importbin "test_data.bin";
bytes[0] == 72 && bytes[1] == 101 && bytes[2] == 108 && bytes[3] == 108 && bytes[4] == 111); true
