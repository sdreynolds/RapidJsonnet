assert (// Test bitwise precedence: & has higher precedence than ^, which has higher precedence than |
// 1 | 2 ^ 4 & 12 should parse as 1 | (2 ^ (4 & 12))
// 4 & 12 = 4 (binary: 0100 & 1100 = 0100)
// 2 ^ 4 = 6 (binary: 0010 ^ 0100 = 0110)
// 1 | 6 = 7 (binary: 0001 | 0110 = 0111)
1 | 2 ^ 4 & 12) == 7.0; 7.0
