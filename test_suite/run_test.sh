#!/bin/bash
# Test runner for official Jsonnet test suite tests.
#
# Usage: run_test.sh <binary> <test_file> [<golden_file>]
#
# If a golden file is provided, the output is compared against it.
# Otherwise, the test is expected to output "true" (assertEqual-chain pattern).

BIN=$1
FILE=$2
GOLDEN=$3

set +e
# Capture stdout only; discard GC debug noise from stderr
ACTUAL=$($BIN --quiet "$FILE" 2>/dev/null)
EXIT_CODE=$?
set -e

if [ $EXIT_CODE -ne 0 ]; then
    echo "!!! TEST FAILED: $FILE exited with code $EXIT_CODE" >&2
    # Re-run to show error output
    $BIN --quiet "$FILE" >&2 2>&1 || true
    exit 1
fi

if [ -n "$GOLDEN" ] && [ -f "$GOLDEN" ]; then
    EXPECTED=$(cat "$GOLDEN")
    if [ "$ACTUAL" = "$EXPECTED" ]; then
        echo "--- TEST PASSED: $FILE (matched golden)"
        exit 0
    else
        echo "!!! TEST FAILED: $FILE output does not match golden file" >&2
        echo "=== EXPECTED ===" >&2
        echo "$EXPECTED" >&2
        echo "=== ACTUAL ===" >&2
        echo "$ACTUAL" >&2
        diff <(echo "$EXPECTED") <(echo "$ACTUAL") >&2 || true
        exit 1
    fi
else
    # No golden file — expect output to be exactly "true"
    if [ "$ACTUAL" = "true" ]; then
        echo "--- TEST PASSED: $FILE"
        exit 0
    else
        echo "!!! TEST FAILED: $FILE expected 'true' but got:" >&2
        echo "$ACTUAL" >&2
        exit 1
    fi
fi
