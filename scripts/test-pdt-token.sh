#!/bin/bash
set -e

# ==========================================
#  PDT API OAuth2 Token Test
# ==========================================
#
# Usage:
#   source .env && bash scripts/test-pdt-token.sh
# Or:
#   AUTH_ISSUER_URL=... AUTH_PDT_URL=... bash scripts/test-pdt-token.sh

# Load .env if present (skip if already set)
if [ -f ".env" ] && [ -z "$AUTH_ISSUER_URL" ]; then
  set -a
  source .env
  set +a
fi

# Required: OIDC issuer URL (e.g. https://idm.tanbal.ir/oauth2/openid/pdt-api)
ISSUER_URL="${AUTH_ISSUER_URL:?AUTH_ISSUER_URL is not set. Source .env or export it.}"

# Derive the base TIDP URL and token endpoint from the issuer
# AUTH_ISSUER_URL = https://idm.tanbal.ir/oauth2/openid/pdt-api
# We strip the /oauth2/openid/pdt-api part to get the TIDP base
TIDP_BASE=$(echo "$ISSUER_URL" | sed 's|/oauth2/openid/.*$||')
TOKEN_ENDPOINT="${TIDP_BASE}/oauth2/token"
AUTHORIZE_ENDPOINT="${TIDP_BASE}/ui/oauth2"
CLIENT_ID=$(basename "$ISSUER_URL")

# Optional: PDT API URL (default to localhost:8080)
PDT_URL="${AUTH_PDT_URL:-http://127.0.0.1:8080}"

# Step 1: Generate PKCE
VERIFIER="3-nc1pyVTklc1AU2IRCY53jiyQnS-wSCkbmtBW4P1jc"
CHALLENGE=$(printf '%s' "$VERIFIER" | openssl dgst -sha256 -binary | openssl base64 -A | tr '+/' '-_' | tr -d '=')

echo "=========================================="
echo "  PDT API OAuth2 Token Test"
echo "=========================================="
echo ""
echo "Issuer:    $ISSUER_URL"
echo "Client:    $CLIENT_ID"
echo "PDT URL:   $PDT_URL"
echo "PKCE Verifier:  $VERIFIER"
echo "PKCE Challenge: $CHALLENGE"
echo ""
echo "Step 1: Open this URL in browser:"
echo "${AUTHORIZE_ENDPOINT}?client_id=${CLIENT_ID}&redirect_uri=http%3A%2F%2Flocalhost%3A8000%2Fpdt%2Fcallback&response_type=code&scope=openid+profile+email&code_challenge=${CHALLENGE}&code_challenge_method=S256"
echo ""
echo "After login, grab the 'code' from the redirect URL."
echo ""
read -p "Paste the code here: " CODE

echo ""
echo "Step 2: Exchanging code for token..."

RESPONSE=$(curl -s -w '\n%{http_code}' \
  -X POST "${TOKEN_ENDPOINT}" \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d "grant_type=authorization_code" \
  -d "code=${CODE}" \
  -d "client_id=${CLIENT_ID}" \
  -d "redirect_uri=http://localhost:8000/pdt/callback" \
  -d "code_verifier=${VERIFIER}")

HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
BODY=$(echo "$RESPONSE" | head -n-1)

echo "HTTP Status: $HTTP_CODE"
echo "Response:"
echo "$BODY" | python3 -m json.tool 2>/dev/null || echo "$BODY"

if [ "$HTTP_CODE" = "200" ]; then
  ACCESS_TOKEN=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('access_token',''))" 2>/dev/null)
  if [ -n "$ACCESS_TOKEN" ]; then
    echo ""
    echo "=========================================="
    echo "Step 3: Testing PDT API with token..."
    echo "=========================================="
    curl -s "${PDT_URL}/api/assets" \
      -H "Authorization: Bearer ${ACCESS_TOKEN}" | python3 -m json.tool 2>/dev/null || echo "PDT request failed"
  fi
fi
