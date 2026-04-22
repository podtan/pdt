#!/bin/bash
# ==========================================
#  inspect-tokens.sh
# ==========================================
# Exchanges the Kanidm service account API token for OIDC tokens
# via RFC 8693 Token Exchange, then prints a detailed summary of
# both the access token and ID token payloads.
#
# Usage:
#   bash scripts/inspect-tokens.sh
#   bash scripts/inspect-tokens.sh --env .env.test-cedar
#
# Requirements:
#   - jq  (apt install jq / brew install jq)
#   - python3 with base64 support (standard)
#
# No proxy is used — direct connection to Kanidm.

set -euo pipefail

# ---------- env file ----------
ENV_FILE="${1:-.env.test-cedar}"

if [ ! -f "$ENV_FILE" ]; then
  echo "ERROR: $ENV_FILE not found"
  echo "Usage: bash scripts/inspect-tokens.sh [--env <file>]"
  exit 1
fi

# Source the env file (use a subshell to avoid polluting caller's env)
eval "$(grep -E '^(KANIDM_SERVICE_TOKEN|KANIDM_TOKEN_AUDIENCE|KANIDM_TOKEN_ENDPOINT)=' "$ENV_FILE")"

# ---------- validate ----------
if [ -z "${KANIDM_SERVICE_TOKEN:-}" ]; then
  echo "ERROR: KANIDM_SERVICE_TOKEN is not set in $ENV_FILE"
  exit 1
fi
if [ -z "${KANIDM_TOKEN_AUDIENCE:-}" ]; then
  echo "ERROR: KANIDM_TOKEN_AUDIENCE is not set in $ENV_FILE"
  exit 1
fi
if [ -z "${KANIDM_TOKEN_ENDPOINT:-}" ]; then
  echo "ERROR: KANIDM_TOKEN_ENDPOINT is not set in $ENV_FILE"
  exit 1
fi

# Derive issuer and userinfo URLs from the token endpoint
# KANIDM_TOKEN_ENDPOINT = https://idm.tanbal.ir/oauth2/token
# Issuer                 = https://idm.tanbal.ir/oauth2/openid/<audience>
# Userinfo               = https://idm.tanbal.ir/oauth2/openid/<audience>/userinfo
IDM_BASE=$(echo "$KANIDM_TOKEN_ENDPOINT" | sed 's|/oauth2/token$||')
ISSUER="${IDM_BASE}/oauth2/openid/${KANIDM_TOKEN_AUDIENCE}"
USERINFO_URL="${ISSUER}/userinfo"

echo ""
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║           Kanidm Token Exchange — Token Inspector              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "  Env file      : $ENV_FILE"
echo "  Audience      : $KANIDM_TOKEN_AUDIENCE"
echo "  Token endpoint: $KANIDM_TOKEN_ENDPOINT"
echo "  Issuer        : $ISSUER"
echo "  Userinfo URL  : $USERINFO_URL"
echo ""

# ---------- Step 1: Decode the service account API token ----------
echo "━━━ STEP 1: Service Account API Token ━━━"
echo ""

SA_PAYLOAD=$(echo "$KANIDM_SERVICE_TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null)

if [ -n "$SA_PAYLOAD" ]; then
  echo "  Payload (decoded):"
  echo "$SA_PAYLOAD" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  (raw) $SA_PAYLOAD"
else
  echo "  (could not decode — may be opaque or HS256 signed)"
fi

echo ""

# ---------- Step 2: Token exchange (RFC 8693) ----------
echo "━━━ STEP 2: RFC 8693 Token Exchange ━━━"
echo ""

EXCHANGE_RESPONSE=$(curl -s \
  -X POST "$KANIDM_TOKEN_ENDPOINT" \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d "grant_type=urn:ietf:params:oauth:grant-type:token-exchange" \
  -d "client_id=${KANIDM_TOKEN_AUDIENCE}" \
  -d "subject_token=${KANIDM_SERVICE_TOKEN}" \
  -d "subject_token_type=urn:ietf:params:oauth:token-type:access_token" \
  -d "audience=${KANIDM_TOKEN_AUDIENCE}" \
  -d "scope=email groups openid profile")

# Check for errors
if echo "$EXCHANGE_RESPONSE" | grep -q '"error"'; then
  echo "  ❌ Token exchange FAILED:"
  echo "$EXCHANGE_RESPONSE" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  $EXCHANGE_RESPONSE"
  echo ""
  exit 1
fi

echo "  ✅ Token exchange succeeded"
echo ""

# Extract tokens
ACCESS_TOKEN=$(echo "$EXCHANGE_RESPONSE" | jq -r '.access_token // empty')
ID_TOKEN=$(echo "$EXCHANGE_RESPONSE" | jq -r '.id_token // empty')
REFRESH_TOKEN=$(echo "$EXCHANGE_RESPONSE" | jq -r '.refresh_token // empty')
SCOPE=$(echo "$EXCHANGE_RESPONSE" | jq -r '.scope // empty')
TOKEN_TYPE=$(echo "$EXCHANGE_RESPONSE" | jq -r '.token_type // empty')
EXPIRES_IN=$(echo "$EXCHANGE_RESPONSE" | jq -r '.expires_in // empty')

echo "  token_type  : $TOKEN_TYPE"
echo "  expires_in  : $EXPIRES_IN"
echo "  scope       : $SCOPE"
echo "  refresh     : ${REFRESH_TOKEN:0:20}... (${#REFRESH_TOKEN} chars)"
echo ""

if [ -z "$ACCESS_TOKEN" ]; then
  echo "  ❌ No access_token in response!"
  exit 1
fi

# ---------- Step 3: Decode Access Token ----------
echo "━━━ STEP 3: Access Token (what PDT validates) ━━━"
echo ""

# Decode JWT payload (pad base64url)
AT_HEADER=$(echo "$ACCESS_TOKEN" | cut -d. -f1 | tr '_-' '/+' | base64 -d 2>/dev/null)
AT_PAYLOAD=$(echo "$ACCESS_TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null)

if [ -n "$AT_HEADER" ]; then
  echo "  Header:"
  echo "$AT_HEADER" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  (raw) $AT_HEADER"
  echo ""
fi

if [ -n "$AT_PAYLOAD" ]; then
  echo "  Payload:"
  echo "$AT_PAYLOAD" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  (raw) $AT_PAYLOAD"

  # Highlight key fields
  echo ""
  echo "  ── Key Claims ──"
  AT_SUB=$(echo "$AT_PAYLOAD" | jq -r '.sub // "MISSING"')
  AT_AUD=$(echo "$AT_PAYLOAD" | jq -r '.aud // "MISSING"')
  AT_ISS=$(echo "$AT_PAYLOAD" | jq -r '.iss // "MISSING"')
  AT_EXP=$(echo "$AT_PAYLOAD" | jq -r '.exp // "MISSING"')
  AT_SCOPE=$(echo "$AT_PAYLOAD" | jq -r '.scope // "MISSING"')
  AT_GROUPS=$(echo "$AT_PAYLOAD" | jq -r '.groups // "NOT PRESENT"')
  AT_ROLE=$(echo "$AT_PAYLOAD" | jq -r '.role // "NOT PRESENT"')

  echo "  sub    : $AT_SUB"
  echo "  aud    : $AT_AUD"
  echo "  iss    : $AT_ISS"
  echo "  exp    : $AT_EXP"
  echo "  scope  : $AT_SCOPE"
  echo ""
  echo -n "  groups : "
  if [ "$AT_GROUPS" = "NOT PRESENT" ]; then
    echo "❌ NOT PRESENT in access token"
  else
    echo "✅ $AT_GROUPS"
  fi
  echo -n "  role   : "
  if [ "$AT_ROLE" = "NOT PRESENT" ]; then
    echo "❌ NOT PRESENT in access token"
  else
    echo "✅ $AT_ROLE"
  fi
else
  echo "  (could not decode — may be opaque token)"
fi

echo ""

# ---------- Step 4: Decode ID Token ----------
if [ -n "$ID_TOKEN" ]; then
  echo "━━━ STEP 4: ID Token (what PDT does NOT validate) ━━━"
  echo ""

  IT_HEADER=$(echo "$ID_TOKEN" | cut -d. -f1 | tr '_-' '/+' | base64 -d 2>/dev/null)
  IT_PAYLOAD=$(echo "$ID_TOKEN" | cut -d. -f2 | tr '_-' '/+' | base64 -d 2>/dev/null)

  if [ -n "$IT_HEADER" ]; then
    echo "  Header:"
    echo "$IT_HEADER" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  (raw) $IT_HEADER"
    echo ""
  fi

  if [ -n "$IT_PAYLOAD" ]; then
    echo "  Payload:"
    echo "$IT_PAYLOAD" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  (raw) $IT_PAYLOAD"

    # Highlight key fields
    echo ""
    echo "  ── Key Claims ──"
    IT_SUB=$(echo "$IT_PAYLOAD" | jq -r '.sub // "MISSING"')
    IT_AUD=$(echo "$IT_PAYLOAD" | jq -r '.aud // "MISSING"')
    IT_NAME=$(echo "$IT_PAYLOAD" | jq -r '.name // "NOT PRESENT"')
    IT_USERNAME=$(echo "$IT_PAYLOAD" | jq -r '.preferred_username // "NOT PRESENT"')
    IT_EMAIL=$(echo "$IT_PAYLOAD" | jq -r '.email // "NOT PRESENT"')
    IT_GROUPS=$(echo "$IT_PAYLOAD" | jq -r '.groups // "NOT PRESENT"')
    IT_ROLE=$(echo "$IT_PAYLOAD" | jq -r '.role // "NOT PRESENT"')
    IT_SCOPES=$(echo "$IT_PAYLOAD" | jq -r '.scopes // "NOT PRESENT"')

    echo "  sub               : $IT_SUB"
    echo "  aud               : $IT_AUD"
    echo "  name              : $IT_NAME"
    echo "  preferred_username: $IT_USERNAME"
    echo "  email             : $IT_EMAIL"
    echo ""
    echo -n "  groups            : "
    if [ "$IT_GROUPS" = "NOT PRESENT" ]; then
      echo "❌ NOT PRESENT"
    else
      echo "✅ $IT_GROUPS"
    fi
    echo -n "  role              : "
    if [ "$IT_ROLE" = "NOT PRESENT" ]; then
      echo "❌ NOT PRESENT"
    else
      echo "✅ $IT_ROLE"
    fi
    echo -n "  scopes (array)    : "
    if [ "$IT_SCOPES" = "NOT PRESENT" ]; then
      echo "NOT PRESENT"
    else
      echo "$IT_SCOPES"
    fi
  else
    echo "  (could not decode — may be opaque token)"
  fi
else
  echo "━━━ STEP 4: ID Token ━━━"
  echo "  (no id_token in response)"
fi

echo ""

# ---------- Step 5: Call Userinfo ----------
echo "━━━ STEP 5: Userinfo Endpoint ━━━"
echo ""

USERINFO_RESPONSE=$(curl -s "$USERINFO_URL" -H "Authorization: Bearer $ACCESS_TOKEN")

if echo "$USERINFO_RESPONSE" | jq -e '.error' >/dev/null 2>&1; then
  echo "  ❌ Userinfo call FAILED:"
  echo "$USERINFO_RESPONSE" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  $USERINFO_RESPONSE"
elif [ -z "$USERINFO_RESPONSE" ] || [ "$USERINFO_RESPONSE" = "{}" ]; then
  echo "  ⚠️  Userinfo returned empty response"
else
  echo "  ✅ Userinfo call succeeded"
  echo ""
  echo "  Response:"
  echo "$USERINFO_RESPONSE" | python3 -m json.tool 2>/dev/null | sed 's/^/  /' || echo "  $USERINFO_RESPONSE"

  echo ""
  echo "  ── Key Claims ──"
  UI_SUB=$(echo "$USERINFO_RESPONSE" | jq -r '.sub // "MISSING"')
  UI_GROUPS=$(echo "$USERINFO_RESPONSE" | jq -r '.groups // "NOT PRESENT"')
  UI_ROLE=$(echo "$USERINFO_RESPONSE" | jq -r '.role // "NOT PRESENT"')
  UI_EMAIL=$(echo "$USERINFO_RESPONSE" | jq -r '.email // "NOT PRESENT"')
  UI_NAME=$(echo "$USERINFO_RESPONSE" | jq -r '.name // "NOT PRESENT"')

  echo "  sub    : $UI_SUB"
  echo "  email  : $UI_EMAIL"
  echo "  name   : $UI_NAME"
  echo -n "  groups : "
  if [ "$UI_GROUPS" = "NOT PRESENT" ]; then
    echo "❌ NOT PRESENT"
  else
    echo "✅ $UI_GROUPS"
  fi
  echo -n "  role   : "
  if [ "$UI_ROLE" = "NOT PRESENT" ]; then
    echo "❌ NOT PRESENT"
  else
    echo "✅ $UI_ROLE"
  fi
fi

echo ""

# ---------- Summary ----------
echo "╔════════════════════════════════════════════════════════════════╗"
echo "║                         SUMMARY                                ║"
echo "╠════════════════════════════════════════════════════════════════╣"
echo "║  Token        │ groups            │ role                       ║"
echo "╠════════════════════════════════════════════════════════════════╣"

# Access token status
if [ "${AT_GROUPS:-}" = "NOT PRESENT" ]; then
  AT_GROUPS_STATUS="❌ no"
else
  AT_GROUPS_STATUS="✅ yes"
fi
if [ "${AT_ROLE:-}" = "NOT PRESENT" ]; then
  AT_ROLE_STATUS="❌ no"
else
  AT_ROLE_STATUS="✅ yes"
fi
printf "║  Access Token │ %-18s │ %-25s ║\n" "$AT_GROUPS_STATUS" "$AT_ROLE_STATUS"

# ID token status
if [ "${IT_GROUPS:-NOT PRESENT}" = "NOT PRESENT" ]; then
  IT_GROUPS_STATUS="❌ no"
else
  IT_GROUPS_STATUS="✅ yes"
fi
if [ "${IT_ROLE:-NOT PRESENT}" = "NOT PRESENT" ]; then
  IT_ROLE_STATUS="❌ no"
else
  IT_ROLE_STATUS="✅ yes"
fi
printf "║  ID Token     │ %-18s │ %-25s ║\n" "$IT_GROUPS_STATUS" "$IT_ROLE_STATUS"

# Userinfo status
if [ "${UI_GROUPS:-NOT PRESENT}" = "NOT PRESENT" ]; then
  UI_GROUPS_STATUS="❌ no"
else
  UI_GROUPS_STATUS="✅ yes"
fi
if [ "${UI_ROLE:-NOT PRESENT}" = "NOT PRESENT" ]; then
  UI_ROLE_STATUS="❌ no"
else
  UI_ROLE_STATUS="✅ yes"
fi
printf "║  Userinfo     │ %-18s │ %-25s ║\n" "$UI_GROUPS_STATUS" "$UI_ROLE_STATUS"

echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Assess the situation
if [ "$AT_GROUPS_STATUS" = "✅ yes" ]; then
  echo "  🎉 groups are in the ACCESS TOKEN — PDT Cedar can use them directly!"
else
  echo "  ⚠️  groups are NOT in the access token."
  echo ""
  echo "  PDT validates the access token, so groups/role are invisible to Cedar."
  echo "  Next steps to fix this:"
  echo ""
  echo "  Option A: Call /userinfo endpoint after JWT validation (recommended, standard OIDC)"
  echo "  Option B: Configure Kanidm supplemental scope maps (no code HTTP calls, parse scope string)"
  echo "  Option C: Validate the ID token instead (non-standard, not recommended)"
fi

echo ""
