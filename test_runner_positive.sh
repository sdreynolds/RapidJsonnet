#!/bin/bash

# Positive tests runner - tests that should PASS
# Runs all .jsonnet files except those with error/wrong/unmatched/unexpected/missing in name

set -e

RUNFILES_DIR="$(pwd)"
JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"

if [ ! -f "$JSONNET_INTERPRETER_PATH" ]; then
    echo "ERROR: Jsonnet interpreter (:main) not found at $JSONNET_INTERPRETER_PATH" >&2
    exit 1
fi

ALL_PASS=true

# Find all positive test files (excluding error/wrong/unmatched/unexpected/missing and negative tests)
# Exclude: _error, _wrong, _unmatched, _unexpected, _missing, std_substr_negative_* (which test invalid inputs)
# Also exclude: function_simple and test_function_basic (known issue: can't serialize functions to JSON)
JSONNET_FILES=$(find "$RUNFILES_DIR/end2end/" -name "*.jsonnet" | grep -v "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative\|function_simple\|test_function_basic" | sort)

if [ -z "$JSONNET_FILES" ]; then
    echo "WARNING: No positive .jsonnet files found to test."
    exit 0
fi

echo "Found positive Jsonnet files:"
echo "$JSONNET_FILES"
echo ""

for jsonnet_file in $JSONNET_FILES; do
    # Pass the absolute path to the Jsonnet tool
    if ! "$JSONNET_INTERPRETER_PATH" "$jsonnet_file" > /dev/null 2>&1; then
        echo "!!! POSITIVE TEST FAILED for $jsonnet_file !!!" >&2
        ALL_PASS=false
    else
        echo "--- POSITIVE TEST PASSED for $jsonnet_file ---"
    fi
done

if [ "$ALL_PASS" = true ]; then
    echo "All positive Jsonnet files passed validation."
    exit 0
else
    echo "One or more positive Jsonnet files failed validation." >&2
    exit 1
fi
