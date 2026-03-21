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

# Determine ext-var flags based on the test file
BASENAME=$(basename "$FILE")
EXT_ARGS=""
if [ "$BASENAME" = "stdlib.jsonnet" ]; then
    EXT_ARGS="--ext-str var1=test --ext-code var2={x:1,y:2}"
fi
# Also check for .jsonnet.in file which may specify ext vars
IN_FILE="${FILE}.in"
if [ -f "$IN_FILE" ]; then
    while IFS= read -r line; do
        EXT_ARGS="$EXT_ARGS $line"
    done < "$IN_FILE"
fi

set +e
if [ -n "$GOLDEN" ] && [ -f "$GOLDEN" ]; then
    # Golden tests: capture both stdout and stderr (for TRACE output),
    # but filter out GC debug noise from stress_gc builds.
    ACTUAL=$($BIN --quiet $EXT_ARGS "$FILE" 2>&1)
    EXIT_CODE=$?
    ACTUAL=$(echo "$ACTUAL" | grep -v '^\[MemoryManager\]\|^\[VirtualMachine\]')
else
    # Non-golden tests: capture stdout only; discard stderr
    ACTUAL=$($BIN --quiet $EXT_ARGS "$FILE" 2>/dev/null)
    EXIT_CODE=$?
fi
set -e

if [ $EXIT_CODE -ne 0 ]; then
    echo "!!! TEST FAILED: $FILE exited with code $EXIT_CODE" >&2
    # Re-run to show error output
    $BIN --quiet $EXT_ARGS "$FILE" >&2 2>&1 || true
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
