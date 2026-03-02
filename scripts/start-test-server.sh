#!/usr/bin/env bash
# start-test-server.sh
#
# Starts SupaRust with test env (port 3001, isolated pg-embed dir).
# Run in a separate terminal before `npm test` in test-client/.
#
# Usage:
#   bash scripts/start-test-server.sh
#
# Requires: .env.test at repo root (run gen-env-test.mjs first)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"
ENV_FILE="$ROOT/.env.test"

if [[ ! -f "$ENV_FILE" ]]; then
  echo "[start-test-server] ERROR: $ENV_FILE not found."
  echo "  Run: node scripts/gen-env-test.mjs"
  exit 1
fi

echo "[start-test-server] Loading $ENV_FILE"

# Export all vars from .env.test — these will override .env
# because dotenvy skips vars already present in process env.
set -a
# shellcheck disable=SC1090
source "$ENV_FILE"
set +a

echo "[start-test-server] Starting SupaRust on port $SUPARUST_PORT"
echo "[start-test-server] DB dir: $SUPARUST_DB_DATA_DIR"

cd "$ROOT"
exec cargo run
