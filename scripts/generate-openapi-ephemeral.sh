#!/usr/bin/env bash
#
# generate-openapi-ephemeral.sh — Generate OpenAPI spec using an ephemeral MongoDB.
#
# Spins up a temporary MongoDB container, creates an admin user, runs
# generate-openapi.sh to extract the spec, then tears down the container.
#
# No external dependencies required beyond podman.
#
# Usage: ./scripts/generate-openapi-ephemeral.sh [output_path]
#
set -euo pipefail

cd "$(dirname "$0")/.."

CONTAINER_NAME="pdt-mongo-openapi"
MONGO_IMAGE="docker.io/library/mongo:7"
OUTPUT_FILE="${1:-$(pwd)/docs/openapi/pdt-openapi.json}"

# ---------------------------------------------------------------------------
# 1. Start ephemeral MongoDB
# ---------------------------------------------------------------------------
echo "» Starting ephemeral MongoDB (${CONTAINER_NAME})…"
podman rm -f "$CONTAINER_NAME" &>/dev/null || true
podman run -d --name "$CONTAINER_NAME" -p 27017:27017 "$MONGO_IMAGE" &>/dev/null

cleanup() {
    echo "» Removing ephemeral MongoDB (${CONTAINER_NAME})…"
    podman rm -f "$CONTAINER_NAME" &>/dev/null || true
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# 2. Wait for MongoDB to be ready, then create admin user
# ---------------------------------------------------------------------------
echo "» Waiting for MongoDB to be ready…"
for i in $(seq 1 30); do
    if podman exec "$CONTAINER_NAME" mongosh --quiet --eval 'db.runCommand({ping:1})' &>/dev/null; then
        echo "✓ MongoDB is ready (attempt ${i})"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "✗ MongoDB did not come up in time."
        exit 1
    fi
    sleep 1
done

echo "» Creating admin user…"
podman exec "$CONTAINER_NAME" mongosh --quiet \
    --eval 'db.getSiblingDB("admin").createUser({user:"admin", pwd:"password", roles:["root"]})' \
    >/dev/null 2>&1

# ---------------------------------------------------------------------------
# 3. Generate OpenAPI spec
# ---------------------------------------------------------------------------
echo "» Generating OpenAPI spec…"
./scripts/generate-openapi.sh "$OUTPUT_FILE"

# ---------------------------------------------------------------------------
# 4. Cleanup (handled by trap on EXIT)
# ---------------------------------------------------------------------------
echo "✓ Done. Spec at: ${OUTPUT_FILE}"
