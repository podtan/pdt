#!/usr/bin/env bash
#
# generate-openapi.sh — Build PDT, start it locally, extract OpenAPI spec.
#
# Usage: ./scripts/generate-openapi.sh [output_path]
#
# Defaults:
#   - port:     8087 (override with PDT_PORT env var)
#   - output:   docs/openapi/pdt-openapi.json in repo root
#
# Environment:
#   PDT_PORT          — port to run PDT on (default: 8087)
#   DOCUMENTDB_URL    — MongoDB connection URL
#   DOCUMENTDB_USERNAME — MongoDB username
#   DOCUMENTDB_PASSWORD — MongoDB password
#   DOCUMENTDB_DATABASE — MongoDB database name
#
set -euo pipefail

cd "$(dirname "$0")/.."

PORT="${PDT_PORT:-8087}"
HOST="127.0.0.1"
BASE_URL="http://${HOST}:${PORT}"
OUTPUT_FILE="${1:-$(pwd)/docs/openapi/pdt-openapi.json}"
PID_FILE="/tmp/pdt-openapi.pid"

# ---------------------------------------------------------------------------
# 1. Build release binary
# ---------------------------------------------------------------------------
echo "» Building pdt (release)…"
cargo build --release 2>&1 | tail -3

# ---------------------------------------------------------------------------
# 2. Start pdt in background with dev-mode auth and Cedar disabled
# ---------------------------------------------------------------------------
echo "» Starting pdt on port ${PORT}…"

# Use a dev .env if none exists; the server just needs to boot to serve the spec
export PDT_HOST="${HOST}"
export PDT_PORT="${PORT}"
export RUST_LOG="${RUST_LOG:-warn}"
export AUTH_ENABLED="${AUTH_ENABLED:-false}"
export AUTH_DEV_MODE="${AUTH_DEV_MODE:-true}"
export CEDAR_ENABLED="${CEDAR_ENABLED:-false}"
export DOCUMENTDB_URL="${DOCUMENTDB_URL:-mongodb://127.0.0.1:27017}"
export DOCUMENTDB_USERNAME="${DOCUMENTDB_USERNAME:-admin}"
export DOCUMENTDB_PASSWORD="${DOCUMENTDB_PASSWORD:-password}"
export DOCUMENTDB_DATABASE="${DOCUMENTDB_DATABASE:-pdt}"

./target/release/pdt &>/tmp/pdt-openapi.log &
PDT_PID=$!
echo "$PDT_PID" > "$PID_FILE"

cleanup() {
    if kill -0 "$PDT_PID" 2>/dev/null; then
        echo "» Stopping pdt (PID ${PDT_PID})…"
        kill "$PDT_PID" 2>/dev/null || true
        wait "$PDT_PID" 2>/dev/null || true
    fi
    rm -f "$PID_FILE"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# 3. Wait for server to come up (health check)
# ---------------------------------------------------------------------------
echo "» Waiting for pdt on ${BASE_URL}…"
for i in $(seq 1 30); do
    if curl -sf "${BASE_URL}/health" >/dev/null 2>&1; then
        echo "✓ Server is up (attempt ${i})"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "✗ pdt did not come up in time. Last log lines:"
        tail -20 /tmp/pdt-openapi.log
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# 4. Fetch the OpenAPI spec
# ---------------------------------------------------------------------------
echo "» Fetching OpenAPI spec from ${BASE_URL}/api-docs/openapi.json …"

# Ensure output directory exists
mkdir -p "$(dirname "$OUTPUT_FILE")"

curl -sf "${BASE_URL}/api-docs/openapi.json" | python3 -m json.tool > "$OUTPUT_FILE"

PATH_COUNT=$(python3 -c "import json; d=json.load(open('${OUTPUT_FILE}')); print(len(d.get('paths',{})))")
SCHEMA_COUNT=$(python3 -c "import json; d=json.load(open('${OUTPUT_FILE}')); print(len(d.get('components',{}).get('schemas',{})))")

echo ""
echo "✓ OpenAPI spec saved to: ${OUTPUT_FILE}"
echo "  Paths:   ${PATH_COUNT}"
echo "  Schemas: ${SCHEMA_COUNT}"
echo "  Size:    $(wc -c < "$OUTPUT_FILE") bytes"
