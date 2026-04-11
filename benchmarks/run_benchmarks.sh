#!/bin/bash
set -e

HYPERFINE_BIN=$1
MAIN_BIN=$2
GOOGLE_BIN=$3
GO_BIN=$4
# To add another implementation: add it here as $5 and increment shift to 5
shift 4

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

    # Always benchmark RapidJsonnet
    hyperfine_args=(
      "-w" "3"
      "--export-markdown" "$OUT_DIR/$filename-results.md"
      "-n" "RapidJsonnet: $filename" "$MAIN_BIN -q $item"
    )

    # Conditionally benchmark GoogleJsonnet (C++)
    case "$filename" in
        "bench.07.jsonnet" | \
        "bench.09.jsonnet" | \
        "realistic_1.jsonnet" | \
        "realistic_2.jsonnet" | \
        "std_base64.jsonnet" | \
        "std_base64Decode.jsonnet" | \
        "std_base64DecodeBytes.jsonnet" | \
        "comparison_array.jsonnet" | \
        "comparison_primitives.jsonnet")
            echo "Skipping GoogleJsonnet for $filename (known performance/stability issue)"
            ;;
        *)
            hyperfine_args+=("-n" "GoogleJsonnet: $filename" "$GOOGLE_BIN $item")
            ;;
    esac

    # Conditionally benchmark GoJsonnet
    # Note: large_string_template.jsonnet crashes Go (OS stack exhaustion) — if it's
    # ever added to benchmarks/extra/, add it to the skip list below.
    case "$filename" in
        "bench.07.jsonnet")
            echo "Skipping GoJsonnet for $filename (max stack frames exceeded)"
            ;;
        *) hyperfine_args+=("-n" "GoJsonnet: $filename" "$GO_BIN $item") ;;
    esac

    $HYPERFINE_BIN "${hyperfine_args[@]}"
done

echo "=== BENCHMARK RESULTS SAVED TO $OUT_DIR/ ==="
