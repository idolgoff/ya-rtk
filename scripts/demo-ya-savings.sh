#!/usr/bin/env bash
# Demo token savings for ya / arc filters using real fixtures (no Arcadia mount needed).
# Usage: ./scripts/demo-ya-savings.sh
# Optional: RTK=/path/to/rtk ./scripts/demo-ya-savings.sh

set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -n "${RTK:-}" && -x "$RTK" ]]; then
  :
elif [[ -x "$ROOT/target/debug/rtk" ]]; then
  RTK="$ROOT/target/debug/rtk"
elif [[ -x "$ROOT/target/release/rtk" ]]; then
  RTK="$ROOT/target/release/rtk"
else
  echo "Building rtk…"
  cargo build -q
  # Honor CARGO_TARGET_DIR / cargo metadata when target/ is redirected
  TARGET_DIR=$(cargo metadata --format-version 1 --no-deps 2>/dev/null \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['target_directory'])")
  RTK="$TARGET_DIR/debug/rtk"
fi
if [[ ! -x "$RTK" ]]; then
  echo "error: rtk binary not found at $RTK" >&2
  exit 1
fi

demo() {
  local label="$1" filter="$2" fixture="$3"
  local raw filtered raw_t filt_t pct
  raw=$(wc -c <"$fixture" | tr -d ' ')
  filtered=$("$RTK" pipe -f "$filter" <"$fixture" | wc -c | tr -d ' ')
  raw_t=$((raw / 4))
  filt_t=$((filtered / 4))
  if [[ "$raw_t" -eq 0 ]]; then
    pct=0
  else
    pct=$(python3 -c "print(round(100.0*(1-$filt_t/$raw_t),1))")
  fi
  printf '%-28s  raw=%6d tok  filtered=%5d tok  savings=%5s%%\n' \
    "$label" "$raw_t" "$filt_t" "$pct"
}

echo "=== RTK ya/arc token savings (fixture demos) ==="
echo "rtk: $RTK"
echo "metric: chars/4 · filter: rtk pipe -f …"
echo

demo "G1 py fail"             ya         "$ROOT/tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt"
demo "G3 py large fail"       ya         "$ROOT/tests/fixtures/ya/make_t_py_fail_large_logsdir_chunk_raw.txt"
demo "G5 go fail"             ya         "$ROOT/tests/fixtures/ya/make_t_go_fail_logsdir_chunk_raw.txt"
demo "G7 build/proto"         ya-build   "$ROOT/tests/fixtures/ya/make_python_py_build_large_proto_raw.txt"
demo "G10 ttX py fail"        ya         "$ROOT/tests/fixtures/ya/make_ttX_py_fail_logsdir_chunk_raw.txt"
demo "arc status"             arc-status "$ROOT/tests/fixtures/arc/status_raw.txt"
demo "arc log"                arc-log    "$ROOT/tests/fixtures/arc/log_raw.txt"
demo "arc diff"               arc-diff   "$ROOT/tests/fixtures/arc/diff_raw.txt"

echo
echo "=== Sample filtered G1 (head) ==="
"$RTK" pipe -f ya <"$ROOT/tests/fixtures/ya/make_t_py_fail_logsdir_raw.txt" | head -40
echo "…"
echo
echo "Live agent path: prefer \`rtk ya make -t …\` then \`rtk gain -H\`"
echo "Side-by-side live:  ya make -t PATH  vs  rtk ya make -t PATH"
