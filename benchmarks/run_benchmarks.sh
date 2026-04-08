#!/bin/bash
set -e

HYPERFINE_BIN=$1
MAIN_BIN=$2
shift 2

BENCHMARKS=("$@")

if [ ${#BENCHMARKS[@]} -eq 0 ]; then
    echo "No benchmarks provided!"
    exit 1
fi


# Build the hyperfine command line
# We create a temporary string of files to run.
# Actually, hyperfine takes the command to run.
# We can run hyperfine for each or once for all.
# If we do `hyperfine -n label "cmd {}" -L file a,b,c` it works for all files.

echo "Running benchmarks using hyperfine..."
for item in "${BENCHMARKS[@]}"; do
    filename=$(basename "$item")

    $HYPERFINE_BIN -w 3 --export-markdown "${TEST_UNDECLARED_OUTPUTS_DIR:-.}/$filename-results.md" -n "RapidJsonnet: $filename" "$MAIN_BIN -q $item"
done


echo "=== BENCHMARK RESULTS SAVED TO ${TEST_UNDECLARED_OUTPUTS_DIR:-.}/ ==="
