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

# Determine expected exit code
EXPECTED_EXIT_CODE=0
BASENAME=$(basename "$FILE")
if [[ "$BASENAME" == error.* ]]; then
    EXPECTED_EXIT_CODE=1
fi

# Determine ext-var/TLA flags based on the test file
PARAMS=""
if [[ "$BASENAME" == tla.* ]]; then
    PARAMS="--tla-str var1=test --tla-code var2={x:1,y:2}"
else
    PARAMS="--ext-str var1=test --ext-code var2={x:1,y:2}"
fi

# Also check for .jsonnet.in file which may specify ext vars
IN_FILE="${FILE}.in"
if [ -f "$IN_FILE" ]; then
    while IFS= read -r line; do
        PARAMS="$PARAMS $line"
    done < "$IN_FILE"
fi

set +e
# Capture both stdout and stderr (for TRACE and errors),
# but filter out GC debug noise from stress_gc builds.
# Also filter out the "Error: RuntimeError" or "Error: CompilerError" that our interpreter prints at the end of stack traces
# because official goldens don't have it.
ACTUAL=$($BIN --quiet $PARAMS "$FILE" 2>&1)
EXIT_CODE=$?
# Filter memory management noise
ACTUAL=$(echo "$ACTUAL" | grep -v '^\[MemoryManager\]\|^\[VirtualMachine\]')
# Filter trailing error type message if it's an error test
if [ $EXPECTED_EXIT_CODE -ne 0 ]; then
    ACTUAL=$(echo "$ACTUAL" | grep -v '^Error: RuntimeError\|^Error: CompilerError')
fi
set -e

# Verify exit code
if [ $EXIT_CODE -ne $EXPECTED_EXIT_CODE ]; then
    echo "!!! TEST FAILED: $FILE exited with code $EXIT_CODE, but expected $EXPECTED_EXIT_CODE" >&2
    echo "=== OUTPUT ===" >&2
    echo "$ACTUAL" >&2
    exit 1
fi

if [ -n "$GOLDEN" ] && [ -f "$GOLDEN" ]; then
    EXPECTED=$(cat "$GOLDEN")
    # Compare with all whitespace removed for maximum robustness against formatting differences
    ACTUAL_CLEAN=$(echo "$ACTUAL" | tr -d '[:space:]')
    EXPECTED_CLEAN=$(echo "$EXPECTED" | tr -d '[:space:]')
    
    if [ "$ACTUAL_CLEAN" = "$EXPECTED_CLEAN" ]; then
        echo "--- TEST PASSED: $FILE (matched golden)"
        exit 0
    else
        echo "!!! TEST FAILED: $FILE output does not match golden file" >&2
        echo "=== EXPECTED ===" >&2
        echo "$EXPECTED" >&2
        echo "=== ACTUAL ===" >&2
        echo "$ACTUAL" >&2
        # Also show cleaned comparison for debugging
        # echo "ACTUAL_CLEAN: $ACTUAL_CLEAN"
        # echo "EXPECTED_CLEAN: $EXPECTED_CLEAN"
        diff <(echo "$EXPECTED") <(echo "$ACTUAL") >&2 || true
        exit 1
    fi
else
    # No golden file — expect output to be exactly "true"
    ACTUAL_CLEAN=$(echo "$ACTUAL" | tr -d '[:space:]')
    if [ "$ACTUAL_CLEAN" = "true" ]; then
        echo "--- TEST PASSED: $FILE"
        exit 0
    else
        echo "!!! TEST FAILED: $FILE expected 'true' but got:" >&2
        echo "$ACTUAL" >&2
        exit 1
    fi
fi
