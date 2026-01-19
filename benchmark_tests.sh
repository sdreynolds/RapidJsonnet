#!/bin/bash

# Benchmark script to measure sequential vs concurrent test execution (hq-gipl)
# Runs tests in both modes and displays performance metrics

set -e

JSONNET_INTERPRETER="bazel-bin/main"

if [ ! -f "$JSONNET_INTERPRETER" ]; then
    echo "Building project..."
    bazel build //:main > /dev/null 2>&1
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test Execution Performance Benchmark"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Test 1: Sequential execution (positive then negative)
echo "🔄 Test 1: Sequential Execution (Positive + Negative)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

SEQ_START=$(date +%s%N)

# Run positive tests
JSONNET_FILES=$(find end2end/ -name "*.jsonnet" | grep -v "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative\|function_simple\|test_function_basic" | sort)
for jsonnet_file in $JSONNET_FILES; do
    "$JSONNET_INTERPRETER" "$jsonnet_file" > /dev/null 2>&1
done
echo "✓ Positive tests completed"

# Run negative tests
JSONNET_FILES=$(find end2end/ -name "*.jsonnet" | grep "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative" | sort)
for jsonnet_file in $JSONNET_FILES; do
    "$JSONNET_INTERPRETER" "$jsonnet_file" > /dev/null 2>&1 || true
done
echo "✓ Negative tests completed"

SEQ_END=$(date +%s%N)
SEQ_MS=$(( (SEQ_END - SEQ_START) / 1000000 ))
SEQ_S=$(( SEQ_MS / 1000 ))
SEQ_MS_FRAC=$(( SEQ_MS % 1000 ))

echo "Sequential Total Time: ${SEQ_S}.${SEQ_MS_FRAC}s"
echo ""

# Test 2: Concurrent execution (positive and negative in parallel)
echo "⚡ Test 2: Concurrent Execution (Parallel)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

CONC_START=$(date +%s%N)

# Run positive tests in background
{
    JSONNET_FILES=$(find end2end/ -name "*.jsonnet" | grep -v "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative\|function_simple\|test_function_basic" | sort)
    for jsonnet_file in $JSONNET_FILES; do
        "$JSONNET_INTERPRETER" "$jsonnet_file" > /dev/null 2>&1
    done
} &
POSITIVE_PID=$!

# Run negative tests in background
{
    JSONNET_FILES=$(find end2end/ -name "*.jsonnet" | grep "_error\|_wrong\|_unmatched\|_unexpected\|_missing\|test_error\|test_wrong\|test_unmatched\|test_unexpected\|test_missing\|std_substr_negative" | sort)
    for jsonnet_file in $JSONNET_FILES; do
        "$JSONNET_INTERPRETER" "$jsonnet_file" > /dev/null 2>&1 || true
    done
} &
NEGATIVE_PID=$!

# Wait for both
wait $POSITIVE_PID
wait $NEGATIVE_PID

echo "✓ Positive tests completed (in parallel)"
echo "✓ Negative tests completed (in parallel)"

CONC_END=$(date +%s%N)
CONC_MS=$(( (CONC_END - CONC_START) / 1000000 ))
CONC_S=$(( CONC_MS / 1000 ))
CONC_MS_FRAC=$(( CONC_MS % 1000 ))

echo "Concurrent Total Time: ${CONC_S}.${CONC_MS_FRAC}s"
echo ""

# Calculate speedup
if [ $CONC_MS -gt 0 ]; then
    SPEEDUP=$(( SEQ_MS * 100 / CONC_MS ))
    SPEEDUP_FACTOR=$(( (SPEEDUP + 50) / 100 ))
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Performance Improvement:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Sequential:  ${SEQ_S}.${SEQ_MS_FRAC}s"
    echo "Concurrent:  ${CONC_S}.${CONC_MS_FRAC}s"
    echo "Speedup:     ${SPEEDUP}% (approximately ${SPEEDUP_FACTOR}x)"
    echo "Time Saved:  $(( SEQ_MS - CONC_MS ))ms"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
fi
