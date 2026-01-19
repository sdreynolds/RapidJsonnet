#!/bin/bash

# Capture expected outputs and error messages for all tests (hq-ebal)
# This script runs each test and saves its output/error to a .expected file

set -e

JSONNET_INTERPRETER="$(pwd)/bazel-bin/main"
END2END_DIR="end2end"

if [ ! -f "$JSONNET_INTERPRETER" ]; then
    echo "ERROR: Jsonnet interpreter not found at $JSONNET_INTERPRETER"
    echo "Run: bazel build //:main first"
    exit 1
fi

echo "Capturing expected outputs for all tests..."
echo ""

# Process all .jsonnet files
for jsonnet_file in $(find "$END2END_DIR" -name "*.jsonnet" | sort); do
    expected_file="${jsonnet_file%.jsonnet}.expected"

    # Check if it's a negative test (should fail)
    if [[ "$jsonnet_file" == *"error"* ]] || [[ "$jsonnet_file" == *"wrong"* ]] || \
       [[ "$jsonnet_file" == *"unmatched"* ]] || [[ "$jsonnet_file" == *"unexpected"* ]] || \
       [[ "$jsonnet_file" == *"missing"* ]]; then
        # Negative test - capture error message
        echo "Capturing error for: $jsonnet_file"
        if "$JSONNET_INTERPRETER" "$jsonnet_file" > /tmp/output.txt 2>&1; then
            # Test didn't fail when it should have
            echo "  WARNING: Test should have failed but passed!"
        else
            # Extract just the error message line
            grep "^Error:" /tmp/output.txt | head -1 > "$expected_file" || \
            grep "Error:" /tmp/output.txt | head -1 > "$expected_file" || \
            tail -1 /tmp/output.txt > "$expected_file"
        fi
    else
        # Positive test - capture execution result
        echo "Capturing output for: $jsonnet_file"
        if "$JSONNET_INTERPRETER" "$jsonnet_file" > /tmp/output.txt 2>&1; then
            # Extract the execution result line
            grep "🎯 Execution result:" /tmp/output.txt | sed 's/.*🎯 Execution result: //' > "$expected_file" || \
            tail -1 /tmp/output.txt > "$expected_file"
        else
            # Test failed when it shouldn't have
            echo "  WARNING: Test failed when it should have passed!"
            tail -1 /tmp/output.txt > "$expected_file"
        fi
    fi
done

echo ""
echo "Expected output capture complete!"
ls -la "$END2END_DIR"/*.expected | wc -l
echo "expected files created/updated"
