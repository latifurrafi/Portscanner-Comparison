#!/usr/bin/env bash
# benchmark.sh — Compare available scanner binaries (Go goroutine, Rust)
# Usage: ./benchmark.sh <target-host> [start] [end]
# Example: ./benchmark.sh 127.0.0.1 1 1024
#
# This script will look for:
#  - ./go/portscan-go        (goroutine-based Go scanner)
#  - ./rust/target/release/portscan-rs (Rust scanner)
#
# It requires GNU time (usually /usr/bin/time) or gtime (macOS via brew).
# Optional: jq (for counting JSON results).
set -euo pipefail

TARGET=${1:-}
START=${2:-1}
END=${3:-1024}

if [ -z "$TARGET" ]; then
  echo "Usage: $0 <target-host> [start] [end]"
  exit 2
fi

# Candidate binaries (relative paths)
CANDIDATES=(
  "./go/portscan-go"        # Go goroutine scanner
  "./rust/target/release/portscan-rs"  # Rust scanner
)

# Choose a GNU time binary: prefer gtime (macOS), then /usr/bin/time
TIME_BIN="$(command -v gtime || command -v /usr/bin/time || true)"
if [ -z "$TIME_BIN" ]; then
  echo "Error: GNU time not found. Install 'time' (Linux) or 'gtime' via brew (macOS)."
  exit 2
fi

# Verify that chosen time supports -f by running a tiny check
if ! "$TIME_BIN" -f "%e" --version >/dev/null 2>&1 && ! "$TIME_BIN" -f "%e" echo >/dev/null 2>&1; then
  # Some 'time' variants reject -f; fail early
  echo "Error: $TIME_BIN doesn't support -f format. Please install GNU time (gnu-time)."
  exit 2
fi

# Find which binaries exist and are executable
BINS=()
BIN_NAMES=()
for p in "${CANDIDATES[@]}"; do
  if [ -x "$p" ]; then
    BINS+=("$p")
    # pretty name
    name=$(basename "$p")
    BIN_NAMES+=("$name")
  fi
done

# Probe candidates with --help to ensure they are runnable
RUNNABLE_BINS=()
RUNNABLE_NAMES=()
for i in "${!BINS[@]}"; do
  bin="${BINS[i]}"
  name="${BIN_NAMES[i]}"
  if "$bin" --help >/dev/null 2>&1; then
    RUNNABLE_BINS+=("$bin")
    RUNNABLE_NAMES+=("$name")
  else
    echo "Skipping $name: not runnable (failed --help probe)"
  fi
done
BINS=("${RUNNABLE_BINS[@]}")
BIN_NAMES=("${RUNNABLE_NAMES[@]}")

if [ ${#BINS[@]} -eq 0 ]; then
  echo "No runnable scanner binaries found. Build at least one of:"
  echo "  cd go && go build -ldflags \"-s -w\" -o portscan-go main.go"
  echo "  cd rust && cargo build --release"
  exit 2
fi

echo "Found binaries: ${BIN_NAMES[*]}"
echo "Using time binary: $TIME_BIN"

OUT="benchmark_results.csv"
: > "$OUT"
echo "lang,workers,second,memory_kb" >> "$OUT"

# Scanner behavior flags (tweak as needed)
RETRIES=${RETRIES:-1}
ADAPTIVE_FLAG=${ADAPTIVE_FLAG:---adaptive}

# Workers to test (tune as you like)
WORKER_SET="50 200 500 1000 2000"
REPEATS=3

for idx in "${!BINS[@]}"; do
  BIN="${BINS[idx]}"
  NAME="$(basename "$BIN")"
  # map binary name to language label
  case "$NAME" in
    portscan-go) LANG=go ;;
    portscan-rs) LANG=rust ;;
    *) LANG="$NAME" ;;
  esac
  BIN_SIZE=$(stat -c%s "$BIN" || stat -f%z "$BIN") # portable-ish
  for w in $WORKER_SET; do
    for run in $(seq 1 $REPEATS); do
      echo "Running $NAME (workers=$w) run=$run ..."
      TMP_OUT=$(mktemp)
      TIME_FILE=$(mktemp)

      # Run scanner; redirect JSON/text to TMP_OUT
      # Use --json if supported by binary (we try; if binary doesn't support it, it will ignore or error -> fallback)
      # We wrap in '|| true' to ensure we still capture time info even if the scanner returns non-zero.
      "$TIME_BIN" -f "%e %U %S %M %c" -o "$TIME_FILE" \
        "$BIN" --host "$TARGET" --start "$START" --end "$END" --workers "$w" --timeout 300 "$ADAPTIVE_FLAG" --retries "$RETRIES" --json \
        > "$TMP_OUT" 2>/dev/null || "$TIME_BIN" -f "%e %U %S %M %c" -o "$TIME_FILE" "$BIN" --host "$TARGET" --start "$START" --end "$END" --workers "$w" --timeout 300 "$ADAPTIVE_FLAG" --retries "$RETRIES" > "$TMP_OUT" 2>/dev/null || true

      # Read timing fields
      read wall usr sys maxrss volctx < "$TIME_FILE" || true
      rm -f "$TIME_FILE"

      # Clean up and append to CSV (lang, workers, second, memory_kb)
      rm -f "$TMP_OUT"
      echo "$LANG,$w,$wall,$maxrss" >> "$OUT"
    done
  done
done

echo "✅ Done. Results saved in: $OUT"
echo "Open the CSV to compare. Example:"
echo "  column 'wall_seconds' = elapsed time (lower is better)"
echo "  column 'max_rss_kb' = peak memory in KB (lower is better)"
