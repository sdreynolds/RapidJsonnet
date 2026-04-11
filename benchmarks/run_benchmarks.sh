#!/bin/bash
set -e

HYPERFINE_BIN=$1
MAIN_BIN=$2
GOOGLE_BIN=$3
shift 3

BENCHMARKS=("$@")

if [ ${#BENCHMARKS[@]} -eq 0 ]; then
    echo "No benchmarks provided!"
    exit 1
fi

OUT_DIR="${BUILD_WORKSPACE_DIRECTORY:-.}/benchmark-results"
mkdir -p "$OUT_DIR"

echo "Running benchmarks using hyperfine..."
for item in "${BENCHMARKS[@]}"; do
    filename=$(basename "$item")

    $HYPERFINE_BIN -w 3 \
      --export-markdown "$OUT_DIR/$filename-results.md" \
      -n "RapidJsonnet: $filename" "$MAIN_BIN -q $item" \
      -n "GoogleJsonnet: $filename" "$GOOGLE_BIN $item"
done

echo "=== BENCHMARK RESULTS SAVED TO $OUT_DIR/ ==="
