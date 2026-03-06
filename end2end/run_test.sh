#!/bin/bash
BIN=$1
FILE=$2

echo "Running: $BIN $FILE"

# Determine expected outcome based on filename
EXPECTED_FAIL=0
if [[ "$FILE" == *"wrong"* || "$FILE" == *"unmatched"* || "$FILE" == *"error"* || "$FILE" == *"unexpected"* || "$FILE" == *"missing"* ]]; then
    EXPECTED_FAIL=1
fi

set +e
$BIN $FILE
EXIT_CODE=$?
set -e

if [ $EXIT_CODE -eq 0 ]; then
    if [ "$EXPECTED_FAIL" == "1" ]; then
        echo "!!! TEST FAILED: Expected failure, but it succeeded." >&2
        exit 1
    else
        echo "--- TEST PASSED: Succeeded as expected."
        exit 0
    fi
else
    if [ "$EXPECTED_FAIL" == "1" ]; then
        echo "--- TEST PASSED: Failed as expected (exit code $EXIT_CODE)."
        exit 0
    else
        echo "!!! TEST FAILED: Expected success, but it failed (exit code $EXIT_CODE)." >&2
        exit 1
    fi
fi
