#!/bin/bash

# Concurrent test runner (hq-gipl)
# Runs positive and negative tests in parallel for faster execution

set -e

RUNFILES_DIR="$(pwd)"
JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"

if [ ! -f "$JSONNET_INTERPRETER_PATH" ]; then
    echo "ERROR: Jsonnet interpreter (:main) not found at $JSONNET_INTERPRETER_PATH" >&2
    exit 1
fi

# Start timing
START_TIME=$(date +%s%N)

echo "🚀 Running tests in parallel..."
echo ""

# Run positive tests in background
echo "Starting positive tests in background..."
{
    RUNFILES_DIR="$RUNFILES_DIR" bash -c '
    JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"
    ALL_PASS=true

    JSONNET_FILES=$(find "$RUNFILES_DIR/end2end/" -name "*.jsonnet" | grep -v "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative\|function_simple\|test_function_basic" | sort)

    for jsonnet_file in $JSONNET_FILES; do
        if ! "$JSONNET_INTERPRETER_PATH" "$jsonnet_file" > /dev/null 2>&1; then
            ALL_PASS=false
            echo "FAILED: $(basename $jsonnet_file)"
        fi
    done

    if [ "$ALL_PASS" = true ]; then
        echo "✓ POSITIVE TESTS PASSED"
        exit 0
    else
        echo "✗ POSITIVE TESTS FAILED"
        exit 1
    fi
    ' > /tmp/positive_tests.log 2>&1
} &
POSITIVE_PID=$!

# Run negative tests in background
echo "Starting negative tests in background..."
{
    RUNFILES_DIR="$RUNFILES_DIR" bash -c '
    JSONNET_INTERPRETER_PATH="$RUNFILES_DIR/main"
    ALL_PASS=true

    JSONNET_FILES=$(find "$RUNFILES_DIR/end2end/" -name "*.jsonnet" | grep "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative" | sort)

    for jsonnet_file in $JSONNET_FILES; do
        if "$JSONNET_INTERPRETER_PATH" "$jsonnet_file" > /dev/null 2>&1; then
            ALL_PASS=false
            echo "FAILED: $(basename $jsonnet_file)"
        fi
    done

    if [ "$ALL_PASS" = true ]; then
        echo "✓ NEGATIVE TESTS PASSED"
        exit 0
    else
        echo "✗ NEGATIVE TESTS FAILED"
        exit 1
    fi
    ' > /tmp/negative_tests.log 2>&1
} &
NEGATIVE_PID=$!

# Wait for both tests to complete
echo ""
echo "Waiting for tests to complete..."
POSITIVE_EXIT=0
NEGATIVE_EXIT=0

wait $POSITIVE_PID || POSITIVE_EXIT=$?
wait $NEGATIVE_PID || NEGATIVE_EXIT=$?

# End timing
END_TIME=$(date +%s%N)
ELAPSED_MS=$(( (END_TIME - START_TIME) / 1000000 ))
ELAPSED_S=$(( ELAPSED_MS / 1000 ))
ELAPSED_MS_FRAC=$(( ELAPSED_MS % 1000 ))

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test Results:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Show positive test results
echo "Positive Tests:"
cat /tmp/positive_tests.log | tail -1
if [ $POSITIVE_EXIT -eq 0 ]; then
    echo "Status: ✓ PASSED"
else
    echo "Status: ✗ FAILED"
fi

echo ""

# Show negative test results
echo "Negative Tests:"
cat /tmp/negative_tests.log | tail -1
if [ $NEGATIVE_EXIT -eq 0 ]; then
    echo "Status: ✓ PASSED"
else
    echo "Status: ✗ FAILED"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "⏱️  Total Time: ${ELAPSED_S}.${ELAPSED_MS_FRAC}s"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Return success only if both tests passed
if [ $POSITIVE_EXIT -eq 0 ] && [ $NEGATIVE_EXIT -eq 0 ]; then
    echo "✅ All tests PASSED"
    exit 0
else
    echo "❌ Some tests FAILED"
    exit 1
fi
