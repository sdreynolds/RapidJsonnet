#!/usr/bin/env bash
set -euo pipefail

OUTPUT_FILE="$1"

# Check file exists
if [ ! -f "$OUTPUT_FILE" ]; then
  echo "FAIL: Output file not found: $OUTPUT_FILE"
  exit 1
fi

# Check output is valid JSON
if ! python3 -m json.tool "$OUTPUT_FILE" > /dev/null 2>&1; then
  echo "FAIL: Output is not valid JSON:"
  cat "$OUTPUT_FILE"
  exit 1
fi

# Check content contains expected greeting
if grep -q '"Hello, World!"' "$OUTPUT_FILE"; then
  echo "PASS: Output contains expected greeting"
else
  echo "FAIL: Expected '\"Hello, World!\"' in output, got:"
  cat "$OUTPUT_FILE"
  exit 1
fi
