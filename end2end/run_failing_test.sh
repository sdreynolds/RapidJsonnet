#!/usr/bin/env bash
set -euo pipefail

RUNNER="$1"
TEST_FILE="$2"

# Run the test runner; we expect it to fail (exit 1)
if "$RUNNER" -J . "$TEST_FILE"; then
    echo "ERROR: Expected test runner to exit non-zero, but it succeeded"
    exit 1
fi

echo "PASS: Test runner correctly reported failure"
exit 0
