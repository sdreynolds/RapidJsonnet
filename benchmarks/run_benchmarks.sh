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

# Comma separate benchmark files
printf -v joined '%s,' "${BENCHMARKS[@]}"
BENCH_LIST="${joined%,}"

echo "Running benchmarks using hyperfine..."
$HYPERFINE_BIN --ignore-failure --export-markdown "${TEST_UNDECLARED_OUTPUTS_DIR:-.}/results.md" -n "RapidJsonnet" "$MAIN_BIN -q {file}" -L file "$BENCH_LIST"

echo "=== BENCHMARK RESULTS SAVED TO ${TEST_UNDECLARED_OUTPUTS_DIR:-.}/results.md ==="
