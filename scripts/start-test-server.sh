#!/usr/bin/env bash
# start-test-server.sh
#
# Starts SupaRust with test env. Two modes:
#
#   Default  — SUPARUST_* style, port 53001
#   --compat — Supabase alias style, port 53002 (tests env compat layer)
#
# Usage:
#   bash scripts/start-test-server.sh           # SUPARUST_* style
#   bash scripts/start-test-server.sh --compat  # Supabase compat style
#
# Requires env files (run gen-env-test.mjs first):
#   Default:  .env.test
#   Compat:   .env.supabase.test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# ── Mode selection ────────────────────────────────────────────────────────────
if [[ "${1:-}" == "--compat" ]]; then
  ENV_FILE="$ROOT/.env.supabase.test"
  MODE_LABEL="Supabase compat"
else
  ENV_FILE="$ROOT/.env.test"
  MODE_LABEL="SUPARUST_*"
fi

if [[ ! -f "$ENV_FILE" ]]; then
  echo "[start-test-server] ERROR: $ENV_FILE not found."
  echo "  Run: node scripts/gen-env-test.mjs"
  exit 1
fi

echo "[start-test-server] Mode: $MODE_LABEL"
echo "[start-test-server] Loading $ENV_FILE"

# Export all vars from env file — these win over .env
# because dotenvy skips vars already present in process env.
set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

# Print the port — works for both SUPARUST_PORT and PORT aliases
PORT_VAL="${SUPARUST_PORT:-${PORT:-3000}}"
echo "[start-test-server] Starting SupaRust on port $PORT_VAL"

cd "$ROOT"
exec cargo run -- start
