#!/bin/bash

# Negative tests runner - tests that should FAIL
# Runs all .jsonnet files with error/wrong/unmatched/unexpected/missing in name

set -e

RUNFILES_DIR="$(pwd)"
JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"

if [ ! -f "$JSONNET_INTERPRETER_PATH" ]; then
    echo "ERROR: Jsonnet interpreter (:main) not found at $JSONNET_INTERPRETER_PATH" >&2
    exit 1
fi

ALL_PASS=true

# Find all negative test files (those that should fail)
# Match files with _error, _wrong, _unmatched, _unexpected, _missing, std_substr_negative_*, or test_error, etc. patterns
JSONNET_FILES=$(find "$RUNFILES_DIR/end2end/" -name "*.jsonnet" | grep "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative" | sort)

if [ -z "$JSONNET_FILES" ]; then
    echo "WARNING: No negative .jsonnet files found to test."
    exit 0
fi

echo "Found negative Jsonnet files:"
echo "$JSONNET_FILES"
echo ""

for jsonnet_file in $JSONNET_FILES; do
    # Negative tests should FAIL - success means the interpreter correctly rejects them
    if "$JSONNET_INTERPRETER_PATH" "$jsonnet_file" > /dev/null 2>&1; then
        # Test passed (compiled) but should have failed
        echo "!!! NEGATIVE TEST FAILED (should have errored) for $jsonnet_file !!!" >&2
        ALL_PASS=false
    else
        # Test correctly failed as expected
        echo "--- NEGATIVE TEST PASSED (correctly rejected) for $jsonnet_file ---"
    fi
done

if [ "$ALL_PASS" = true ]; then
    echo "All negative Jsonnet files correctly rejected."
    exit 0
else
    echo "One or more negative Jsonnet files failed validation." >&2
    exit 1
fi
