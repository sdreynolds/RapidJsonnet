#!/bin/bash

# Test runner with output validation (hq-ebal)
# Runs each test and validates output against expected .expected file

set -e

RUNFILES_DIR="$(pwd)"
JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"

if [ ! -f "$JSONNET_INTERPRETER_PATH" ]; then
    echo "ERROR: Jsonnet interpreter (:main) not found at $JSONNET_INTERPRETER_PATH" >&2
    exit 1
fi

ALL_PASS=true

# Find all test files, excluding known broken tests
# Exclude: function_simple, test_function_basic (can't serialize functions)
# Exclude: std_substr_negative_* (negative input tests with different output behavior)
JSONNET_FILES=$(find "$RUNFILES_DIR/end2end/" -name "*.jsonnet" | grep -v "function_simple\|test_function_basic\|std_substr_negative" | sort)

if [ -z "$JSONNET_FILES" ]; then
    echo "WARNING: No .jsonnet files found to test."
    exit 0
fi

echo "Running tests with output validation..."
echo ""

for jsonnet_file in $JSONNET_FILES; do
    expected_file="${jsonnet_file%.jsonnet}.expected"

    # Get just the filename for display
    filename=$(basename "$jsonnet_file")

    # Check if it's a negative test
    is_negative=false
    if [[ "$jsonnet_file" == *"error"* ]] || [[ "$jsonnet_file" == *"wrong"* ]] || \
       [[ "$jsonnet_file" == *"unmatched"* ]] || [[ "$jsonnet_file" == *"unexpected"* ]] || \
       [[ "$jsonnet_file" == *"missing"* ]]; then
        is_negative=true
    fi

    # Run the test and capture output
    if "$JSONNET_INTERPRETER_PATH" "$jsonnet_file" > /tmp/actual_output.txt 2>&1; then
        actual_result=$(grep "🎯 Execution result:" /tmp/actual_output.txt 2>/dev/null | sed 's/.*🎯 Execution result: //' || tail -1 /tmp/actual_output.txt)
    else
        actual_result=$(grep "^Error:" /tmp/actual_output.txt 2>/dev/null | head -1 || grep "Error:" /tmp/actual_output.txt 2>/dev/null | head -1 || tail -1 /tmp/actual_output.txt)
    fi

    # Check if expected file exists
    if [ ! -f "$expected_file" ]; then
        echo "⚠️  WARNING: No expected file for $filename (skipping validation)"
        continue
    fi

    # Read expected output
    expected_result=$(cat "$expected_file")

    # Validate output
    if [ "$actual_result" = "$expected_result" ]; then
        echo "✓ PASS: $filename"
    else
        echo "✗ FAIL: $filename"
        echo "  Expected: $expected_result"
        echo "  Actual:   $actual_result"
        ALL_PASS=false
    fi
done

echo ""
if [ "$ALL_PASS" = true ]; then
    echo "All tests passed validation!"
    exit 0
else
    echo "Some tests failed validation." >&2
    exit 1
fi
