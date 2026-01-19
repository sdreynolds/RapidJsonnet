# Comprehensive Tests for Native String Functions

This document describes the test suite for the three native string functions: `std.substr`, `std.startsWith`, and `std.endsWith`.

## Test Files Organization

All test files are located in the `end2end/` directory and follow the naming convention `std_<function>_<test_case>.jsonnet`.

The test runner (`test_runner.sh`) automatically executes all `.jsonnet` files in this directory.

## std.substr Tests (15 tests)

The `std.substr(str, from, len)` function extracts a substring from a string, starting at position `from` for `len` codepoints.

### Basic Functionality Tests
- **std_substr_basic.jsonnet**: Extract first 5 characters from "hello world" → "hello"
- **std_substr_middle.jsonnet**: Extract substring from middle of string (position 6, length 5) → "world"
- **std_substr_single_char.jsonnet**: Extract single character (position 1, length 1) → "e"
- **std_substr_zero_length.jsonnet**: Extract zero-length substring → ""
- **std_substr_from_zero.jsonnet**: Extract entire string starting at position 0 → "abc"

### Boundary Cases
- **std_substr_full_string.jsonnet**: Extract entire string by specifying exact length
- **std_substr_exceeds_bounds.jsonnet**: Extract with length exceeding remaining string (returns remaining substring)
- **std_substr_at_end.jsonnet**: Extract starting beyond string end (returns empty or error)
- **std_substr_empty_string.jsonnet**: Extract from empty string → ""

### Unicode Support
- **std_substr_unicode.jsonnet**: Extract from string with accented characters "café"
- **std_substr_emoji.jsonnet**: Extract emoji from string "hello😊world"

### Edge Cases
- **std_substr_negative_from.jsonnet**: Negative start offset (error handling expected)
- **std_substr_negative_length.jsonnet**: Negative length (error handling expected)
- **std_substr_large_length.jsonnet**: Length much larger than string (returns remaining substring)
- **std_substr_with_numbers.jsonnet**: Extract from numeric string "0123456789" → "3456"

## std.startsWith Tests (15 tests)

The `std.startsWith(a, b)` function returns `true` if string `a` begins with the prefix string `b`, `false` otherwise.

### Basic Functionality Tests
- **std_startsWith_basic.jsonnet**: String starts with prefix → true
- **std_startsWith_false.jsonnet**: String doesn't start with given prefix → false
- **std_startsWith_single_char.jsonnet**: Single character match at start → true
- **std_startsWith_single_char_false.jsonnet**: Single character mismatch → false

### Boundary Cases
- **std_startsWith_full_string.jsonnet**: Prefix equals entire string → true
- **std_startsWith_empty_prefix.jsonnet**: Empty prefix (all strings start with empty string) → true
- **std_startsWith_empty_string.jsonnet**: Check empty string against prefix → false
- **std_startsWith_both_empty.jsonnet**: Both empty string and prefix empty → true
- **std_startsWith_prefix_longer.jsonnet**: Prefix longer than string → false

### Case Sensitivity
- **std_startsWith_case_sensitive.jsonnet**: Case-sensitive matching ("Hello" ≠ "hello") → false

### Unicode and Special Characters
- **std_startsWith_unicode.jsonnet**: Unicode prefix "café latte" starts with "café" → true
- **std_startsWith_emoji.jsonnet**: Emoji prefix "😊hello" starts with "😊" → true
- **std_startsWith_numbers.jsonnet**: Numeric string "12345" starts with "123" → true
- **std_startsWith_spaces.jsonnet**: String with leading spaces checks prefix → true
- **std_startsWith_special_chars.jsonnet**: Special characters in prefix → true

## std.endsWith Tests (16 tests)

The `std.endsWith(a, b)` function returns `true` if string `a` ends with the suffix string `b`, `false` otherwise.

### Basic Functionality Tests
- **std_endsWith_basic.jsonnet**: String ends with suffix → true
- **std_endsWith_false.jsonnet**: String doesn't end with given suffix → false
- **std_endsWith_single_char.jsonnet**: Single character match at end → true
- **std_endsWith_single_char_false.jsonnet**: Single character mismatch → false

### Boundary Cases
- **std_endsWith_full_string.jsonnet**: Suffix equals entire string → true
- **std_endsWith_empty_suffix.jsonnet**: Empty suffix (all strings end with empty string) → true
- **std_endsWith_empty_string.jsonnet**: Check empty string against suffix → false
- **std_endsWith_both_empty.jsonnet**: Both empty string and suffix empty → true
- **std_endsWith_suffix_longer.jsonnet**: Suffix longer than string → false

### Case Sensitivity
- **std_endsWith_case_sensitive.jsonnet**: Case-sensitive matching ("World" ≠ "world") → false

### Unicode and Special Characters
- **std_endsWith_unicode.jsonnet**: Unicode suffix "hello café" ends with "café" → true
- **std_endsWith_emoji.jsonnet**: Emoji suffix "hello😊" ends with "😊" → true
- **std_endsWith_numbers.jsonnet**: Numeric string "12345" ends with "345" → true
- **std_endsWith_spaces.jsonnet**: String with trailing spaces checks suffix → true
- **std_endsWith_special_chars.jsonnet**: Special characters in suffix (note: reversal test) → false
- **std_endsWith_multiword.jsonnet**: Multi-word suffix matching → true

## Combined/Integration Tests (4 tests)

These tests demonstrate how the three functions can work together:

- **std_combined_substring_checks.jsonnet**: Object containing startsWith, endsWith, and substr results
- **std_combined_extract_substring.jsonnet**: Array of extracted substrings demonstrating sequential extraction
- **std_combined_validation.jsonnet**: Practical use case: password validation using startsWith and endsWith
- **std_combined_string_processing.jsonnet**: Real-world use case: domain name processing

## Expected Behavior Specifications

### std.substr(str, from, len)
- **Input**: Three arguments - string, start position (0-indexed), length
- **Output**: String containing `len` codepoints starting from position `from`
- **Edge Cases**:
  - If `from + len` exceeds string length, return remaining substring from `from`
  - If `from` equals string length, return empty string
  - If `len` is 0, return empty string
  - Handles Unicode properly (counts codepoints, not bytes)

### std.startsWith(a, b)
- **Input**: Two string arguments
- **Output**: Boolean - true if `a` begins with `b`, false otherwise
- **Properties**:
  - Empty string prefix matches any string (returns true)
  - Case-sensitive comparison
  - Works with Unicode and emoji

### std.endsWith(a, b)
- **Input**: Two string arguments
- **Output**: Boolean - true if `a` ends with `b`, false otherwise
- **Properties**:
  - Empty string suffix matches any string (returns true)
  - Case-sensitive comparison
  - Works with Unicode and emoji

## Test Coverage Statistics

- **Total Tests**: 50 native function tests
- **Basic functionality**: 29 tests
- **Edge cases**: 11 tests
- **Unicode/Emoji support**: 6 tests
- **Integration tests**: 4 tests

## Running the Tests

All tests are automatically included when running the validation test suite:

```bash
bazel test //:jsonnet_validation_test
```

Or manually run the interpreter on any test file:

```bash
bazel run //:main -- end2end/std_substr_basic.jsonnet
```

## Implementation Notes

These tests define the expected behavior for native string function implementations. The tests should pass once the following are implemented:

1. Native function registry in `chunk.rs`
2. StdCall opcode handler in `virtual_machine.rs`
3. Compiler integration in `compiler.rs` to emit StdCall opcodes
4. Individual native function implementations following the Crafting Interpreters pattern
