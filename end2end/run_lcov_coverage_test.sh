#!/usr/bin/env bash
set -euo pipefail

RUNNER="$1"
TEST_FILE="$2"

LCOV_OUTPUT=$(mktemp /tmp/coverage_test.XXXXXX.lcov)
trap "rm -f $LCOV_OUTPUT" EXIT

# Run with coverage enabled
"$RUNNER" -J . --coverage --lcov-output "$LCOV_OUTPUT" "$TEST_FILE"

# Assert file was created and is non-empty
if [ ! -s "$LCOV_OUTPUT" ]; then
    echo "FAIL: LCOV output file is missing or empty"
    exit 1
fi

# The test entrypoint file must NOT appear — coverage is only for library deps
if grep -q "SF:.*test_framework_test.jsonnet" "$LCOV_OUTPUT"; then
    echo "FAIL: test entrypoint test_framework_test.jsonnet should be excluded from coverage"
    cat "$LCOV_OUTPUT"
    exit 1
fi

# The imported library must appear (the code under test)
# Path may have a ./ prefix depending on jpath resolution
if ! grep -q "SF:.*import_integration_test.libsonnet" "$LCOV_OUTPUT"; then
    echo "FAIL: expected SF record for import_integration_test.libsonnet (imported file coverage gap)"
    cat "$LCOV_OUTPUT"
    exit 1
fi

# Assert structural LCOV markers are present
if ! grep -q "^DA:" "$LCOV_OUTPUT"; then
    echo "FAIL: no DA (line coverage) records found"
    exit 1
fi

if ! grep -q "^end_of_record" "$LCOV_OUTPUT"; then
    echo "FAIL: no end_of_record markers found"
    exit 1
fi

# Assert at least one line was hit in the imported file
LIBSONNET_SECTION=$(awk '/SF:.*import_integration_test.libsonnet/,/end_of_record/' "$LCOV_OUTPUT")
if ! echo "$LIBSONNET_SECTION" | grep -q "^DA:[0-9]*,1"; then
    echo "FAIL: no lines were marked as hit in import_integration_test.libsonnet"
    echo "Section:"
    echo "$LIBSONNET_SECTION"
    exit 1
fi

echo "PASS: LCOV coverage output is correct and includes imported file spans"
exit 0
