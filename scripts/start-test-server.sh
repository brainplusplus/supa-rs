#!/usr/bin/env bash
# start-test-server.sh
#
# Starts SupaRust with a specific profile.
#
# Usage:
#   bash scripts/start-test-server.sh                        # default .env → profile: local
#   bash scripts/start-test-server.sh --profile test         # .env.test
#   bash scripts/start-test-server.sh --profile supabase.test  # .env.supabase.test (compat)
#   bash scripts/start-test-server.sh --compat               # legacy alias → --profile supabase.test
#
# Requires the env file to exist (run gen-env-test.mjs first).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

# ── Argument parsing ──────────────────────────────────────────────────────────
PROFILE=""
if [[ "${1:-}" == "--profile" && -n "${2:-}" ]]; then
  PROFILE="${2}"
  shift 2
elif [[ "${1:-}" == "--compat" ]]; then
  # Legacy alias kept for backwards compat
  PROFILE="supabase.test"
  shift 1
fi

# ── Validate env file exists ──────────────────────────────────────────────────
if [[ -n "$PROFILE" ]]; then
  ENV_FILE="$ROOT/.env.${PROFILE}"
  if [[ ! -f "$ENV_FILE" ]]; then
    echo "[start-test-server] ERROR: $ENV_FILE not found."
    echo "  Run: node scripts/gen-env-test.mjs"
    exit 1
  fi
  echo "[start-test-server] Profile: ${PROFILE} (${ENV_FILE})"
else
  echo "[start-test-server] Profile: local (default .env)"
fi

# ── Start ─────────────────────────────────────────────────────────────────────
cd "$ROOT"
if [[ -n "$PROFILE" ]]; then
  exec cargo run -- --profile "$PROFILE" start
else
  exec cargo run -- start
fi
