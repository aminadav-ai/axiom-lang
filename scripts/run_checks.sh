#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

mkdir -p logs
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"

# Preserve previous latest.log if it exists (never overwrite experiment logs)
if [[ -f "logs/latest.log" ]]; then
  PREV_MTIME="$(date -r logs/latest.log +%Y%m%d_%H%M%S)"
  mv "logs/latest.log" "logs/archived_${PREV_MTIME}_$$.log"
fi

LOG_FILE="logs/check_${TIMESTAMP}.log"

{
  echo "=== AXIOM-LANG VERIFICATION SUITE (${TIMESTAMP}) ==="
  rustc --version
  cargo --version
  z3 --version

  echo ""
  echo "--- [1/4] Strict Clippy Check (no todo!, no unimplemented!, no dead_code) ---"
  cargo clippy --all-targets -- -D warnings

  echo ""
  echo "--- [2/4] Unit & Z3 Integration Test Suite ---"
  cargo test -- --nocapture

  echo ""
  echo "--- [3/4] End-to-End Verified Graph Ingestion (examples/payment_demo.ax) ---"
  rm -rf .axiom
  cargo run --quiet -- ingest examples/payment_demo.ax
  echo ""
  cargo run --quiet -- graph
  echo ""
  cargo run --quiet -- passport checkout
  rm -rf .axiom

  echo ""
  echo "--- [4/4] Anti-Stub Z3 Rejection Demo (examples/anti_stub_demo.ax) ---"
  if cargo run --quiet -- verify examples/anti_stub_demo.ax; then
    echo "ERROR: anti_stub_demo.ax should have been rejected by Z3!"
    exit 1
  else
    echo "SUCCESS: Z3 caught the fake stub and produced a concrete counterexample."
  fi

  echo ""
  echo "=== ALL CHECKS PASSED ==="
} 2>&1 | tee "$LOG_FILE"

cp "$LOG_FILE" "logs/latest.log"
echo "Saved log to $LOG_FILE"
