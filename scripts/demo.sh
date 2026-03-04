#!/usr/bin/env bash
#
# Bankie Demo Script (Gateway)
# Walks through the full banking lifecycle via the Gateway (:4040) using API key auth.
# All API calls are logged in the portal's API logs.
#
# Auto-creates a portal org + API key if none provided.
#
# Prerequisites:
#   make docker-up   (or make local-setup + make local-gateway)
#
# Usage:
#   ./scripts/demo.sh                          # auto-signup + create key
#   API_KEY=bk_live_... ./scripts/demo.sh      # use existing key
#
# Environment variables:
#   API_KEY          - existing API key (skips auto-signup)
#   GATEWAY_URL      - Gateway URL (default: http://localhost:4040)
#   DEMO_EMAIL       - email for auto-signup (default: demo@bankie.local)
#   DEMO_PASSWORD    - password for auto-signup (default: DemoPass123!)
#   OUTBOX_WAIT      - seconds to wait for outbox processing (default: 15)
#
set -euo pipefail

GATEWAY_URL="${GATEWAY_URL:-http://localhost:4040}"
DEMO_EMAIL="${DEMO_EMAIL:-demo@bankie.local}"
DEMO_PASSWORD="${DEMO_PASSWORD:-DemoPass123!}"
OUTBOX_WAIT="${OUTBOX_WAIT:-15}"
API_KEY="${API_KEY:-}"

CT="Content-Type: application/json"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

step() {
  echo ""
  echo -e "${BLUE}============================================================${NC}"
  echo -e "${BLUE}  STEP: $1${NC}"
  echo -e "${BLUE}============================================================${NC}"
}

info() {
  echo -e "${YELLOW}>>> $1${NC}"
}

success() {
  echo -e "${GREEN}[OK] $1${NC}"
}

pause() {
  echo ""
  echo -e "${YELLOW}(waiting ${1:-3}s for async processing...)${NC}"
  sleep "${1:-3}"
}

# ------------------------------------------------------------------
# Cookie jar for portal session (used only during setup)
# ------------------------------------------------------------------
COOKIE_JAR=$(mktemp /tmp/bankie_demo_cookies.XXXXXX)
trap 'rm -f "$COOKIE_JAR"' EXIT

portal_get_csrf() {
  grep 'csrf_token' "$COOKIE_JAR" 2>/dev/null | awk '{print $NF}' | tail -1
}

# ------------------------------------------------------------------
# STEP 0: Obtain API key (auto-signup or use provided)
# ------------------------------------------------------------------
if [[ -z "$API_KEY" ]]; then
  step "0a. Portal Signup (auto-create org)"

  ORG_NAME="Demo Org $(date +%s)"
  info "Signing up as ${DEMO_EMAIL} (org: ${ORG_NAME})"

  SIGNUP_RESP=$(curl -s -w "\n%{http_code}" -X POST \
    -c "$COOKIE_JAR" \
    -H "$CT" \
    -d "{\"org_name\":\"${ORG_NAME}\",\"name\":\"Demo User\",\"email\":\"${DEMO_EMAIL}\",\"password\":\"${DEMO_PASSWORD}\"}" \
    "${GATEWAY_URL}/portal/v1/auth/signup")
  SIGNUP_STATUS=$(echo "$SIGNUP_RESP" | tail -1)

  if [[ "$SIGNUP_STATUS" =~ ^2 ]]; then
    success "Signed up and logged in."
  elif [[ "$SIGNUP_STATUS" == "409" ]]; then
    info "Org/email already exists, logging in instead..."
    LOGIN_RESP=$(curl -s -w "\n%{http_code}" -X POST \
      -c "$COOKIE_JAR" \
      -H "$CT" \
      -d "{\"email\":\"${DEMO_EMAIL}\",\"password\":\"${DEMO_PASSWORD}\"}" \
      "${GATEWAY_URL}/portal/v1/auth/login")
    LOGIN_STATUS=$(echo "$LOGIN_RESP" | tail -1)
    if [[ "$LOGIN_STATUS" =~ ^2 ]]; then
      success "Logged in."
    else
      echo -e "${RED}Login failed (HTTP ${LOGIN_STATUS}). Check credentials.${NC}"
      echo "$LOGIN_RESP" | sed '$d'
      exit 1
    fi
  else
    echo -e "${RED}Signup failed (HTTP ${SIGNUP_STATUS}).${NC}"
    echo "$SIGNUP_RESP" | sed '$d'
    exit 1
  fi

  step "0b. Create API Key (all scopes)"

  CSRF=$(portal_get_csrf)
  KEY_RESP=$(curl -s -w "\n%{http_code}" -X POST \
    -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H "$CT" -H "X-CSRF-Token: ${CSRF}" \
    -d '{"name":"demo-key","scopes":["accounts:read","accounts:write","ledgers:read","transactions:read","house_accounts:read","house_accounts:write"]}' \
    "${GATEWAY_URL}/portal/v1/api-keys")
  KEY_STATUS=$(echo "$KEY_RESP" | tail -1)
  KEY_BODY=$(echo "$KEY_RESP" | sed '$d')

  if [[ "$KEY_STATUS" =~ ^2 ]]; then
    API_KEY=$(echo "$KEY_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['raw_key'])" 2>/dev/null || echo "")
    if [[ -n "$API_KEY" ]]; then
      success "API key created: ${API_KEY:0:20}..."
      echo "$API_KEY" > .api-key
      info "Key saved to .api-key"
    else
      echo -e "${RED}Failed to extract raw_key from response.${NC}"
      echo "$KEY_BODY" | python3 -m json.tool 2>/dev/null || echo "$KEY_BODY"
      exit 1
    fi
  else
    echo -e "${RED}Failed to create API key (HTTP ${KEY_STATUS}).${NC}"
    echo "$KEY_BODY" | python3 -m json.tool 2>/dev/null || echo "$KEY_BODY"
    exit 1
  fi
else
  info "Using provided API_KEY: ${API_KEY:0:20}..."
fi

AUTH="Authorization: Bearer ${API_KEY}"

# ------------------------------------------------------------------
# STEP 1: Create House Accounts (one per currency)
# House accounts are the bank's own settlement accounts (double-entry counterparty).
# ------------------------------------------------------------------
step "1. Create House Account (USD)"

HOUSE_USD=$(curl -s -X POST "${GATEWAY_URL}/v1/house_account" \
  -H "$AUTH" -H "$CT" \
  -d '{
    "status": "active",
    "account_name": "Master USD Settlement",
    "account_type": "Settlement",
    "currency": "USD"
  }')

echo "$HOUSE_USD" | python3 -m json.tool 2>/dev/null || echo "$HOUSE_USD"
HOUSE_USD_ID=$(echo "$HOUSE_USD" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")

if [[ -n "$HOUSE_USD_ID" ]]; then
  success "House Account (USD) created: $HOUSE_USD_ID"
else
  echo -e "${RED}Failed to create house account. Check server logs.${NC}"
fi

step "1b. Create House Account (TWD)"

HOUSE_TWD=$(curl -s -X POST "${GATEWAY_URL}/v1/house_account" \
  -H "$AUTH" -H "$CT" \
  -d '{
    "status": "active",
    "account_name": "Master TWD Settlement",
    "account_type": "Settlement",
    "currency": "TWD"
  }')

echo "$HOUSE_TWD" | python3 -m json.tool 2>/dev/null || echo "$HOUSE_TWD"

# ------------------------------------------------------------------
# STEP 2: Query House Accounts
# ------------------------------------------------------------------
step "2. List House Accounts"

info "USD house accounts:"
curl -s "${GATEWAY_URL}/v1/house_account?currency=USD" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

info "TWD house accounts:"
curl -s "${GATEWAY_URL}/v1/house_account?currency=TWD" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 3: Open a Bank Account
# Server auto-generates the account ID.
# ------------------------------------------------------------------
step "3. Open Bank Account (USD, Retail/Checking)"

USER_ID="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"

OPEN_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"OpenAccount\": {
      \"account_type\": \"Retail\",
      \"kind\": \"Checking\",
      \"currency\": \"USD\",
      \"external_reference_id\": \"${USER_ID}\"
    }
  }")

echo "$OPEN_RESULT" | python3 -m json.tool 2>/dev/null || echo "$OPEN_RESULT"
ACCOUNT_ID=$(echo "$OPEN_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
ACCOUNT_NUMBER=$(echo "$OPEN_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin).get('account_number',''))" 2>/dev/null || echo "")

if [[ -z "$ACCOUNT_ID" ]]; then
  echo -e "${RED}Failed to open account. Exiting.${NC}"
  exit 1
fi
success "Bank Account opened: $ACCOUNT_ID"
[[ -n "$ACCOUNT_NUMBER" ]] && success "Account Number: $ACCOUNT_NUMBER"

pause 2

# ------------------------------------------------------------------
# STEP 4: Query Bank Account (should be Pending)
# ------------------------------------------------------------------
step "4. Query Bank Account (status should be Pending)"

curl -s "${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 5: Approve Account (KYC)
# This creates the ledger for the account.
# ------------------------------------------------------------------
step "5. Approve Account (KYC) -- creates ledger"

APPROVE_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"ApproveAccount\": {
      \"id\": \"${ACCOUNT_ID}\"
    }
  }")

echo "$APPROVE_RESULT" | python3 -m json.tool 2>/dev/null || echo "$APPROVE_RESULT"
success "Account approved."

pause 2

# ------------------------------------------------------------------
# STEP 6: Query Account Again (should be Approved + has ledger_id)
# ------------------------------------------------------------------
step "6. Query Bank Account (should be Approved with ledger_id)"

ACCOUNT_VIEW=$(curl -s "${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}" -H "$AUTH")
echo "$ACCOUNT_VIEW" | python3 -m json.tool 2>/dev/null
LEDGER_ID=$(echo "$ACCOUNT_VIEW" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")

if [[ -n "$LEDGER_ID" ]]; then
  success "Ledger ID: $LEDGER_ID"
fi

# ------------------------------------------------------------------
# STEP 7: Deposit
# ------------------------------------------------------------------
step "7. Deposit 1000 USD"

DEPOSIT_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"Deposit\": {
      \"id\": \"${ACCOUNT_ID}\",
      \"amount\": {
        \"amount\": \"1000\",
        \"currency\": \"USD\"
      }
    }
  }")

echo "$DEPOSIT_RESULT" | python3 -m json.tool 2>/dev/null || echo "$DEPOSIT_RESULT"
success "Deposit submitted."

info "Waiting for outbox cron to process ledger credit (up to ${OUTBOX_WAIT}s)..."
pause "$OUTBOX_WAIT"

# ------------------------------------------------------------------
# STEP 8: Check Ledger After Deposit
# ------------------------------------------------------------------
step "8. Query Ledger (should show 1000 available)"

if [[ -n "$LEDGER_ID" ]]; then
  curl -s "${GATEWAY_URL}/v1/ledger/${LEDGER_ID}" \
    -H "$AUTH" | python3 -m json.tool 2>/dev/null
else
  info "No ledger_id captured, skipping."
fi

# ------------------------------------------------------------------
# STEP 9: Withdrawal
# ------------------------------------------------------------------
step "9. Withdraw 250 USD"

WITHDRAW_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"Withdrawal\": {
      \"id\": \"${ACCOUNT_ID}\",
      \"amount\": {
        \"amount\": \"250\",
        \"currency\": \"USD\"
      }
    }
  }")

echo "$WITHDRAW_RESULT" | python3 -m json.tool 2>/dev/null || echo "$WITHDRAW_RESULT"
success "Withdrawal submitted."

info "Waiting for outbox cron to process debit-release (up to ${OUTBOX_WAIT}s)..."
pause "$OUTBOX_WAIT"

# ------------------------------------------------------------------
# STEP 10: Check Ledger After Withdrawal
# ------------------------------------------------------------------
step "10. Query Ledger (should show 750 available)"

if [[ -n "$LEDGER_ID" ]]; then
  curl -s "${GATEWAY_URL}/v1/ledger/${LEDGER_ID}" \
    -H "$AUTH" | python3 -m json.tool 2>/dev/null
else
  info "No ledger_id captured, skipping."
fi

# ------------------------------------------------------------------
# STEP 11: Transactions List
# ------------------------------------------------------------------
step "11. List Transactions"

curl -s "${GATEWAY_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID}&offset=0&limit=10" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 12: User View (all accounts + ledgers)
# ------------------------------------------------------------------
step "12. User View (all bank accounts for this user)"

curl -s "${GATEWAY_URL}/v1/user/${USER_ID}" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 13: Open a Second Account (for Transfer)
# ------------------------------------------------------------------
step "13. Open Second Account (USD, Retail/Checking) + Approve + Deposit"

OPEN2_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"OpenAccount\": {
      \"account_type\": \"Retail\",
      \"kind\": \"Checking\",
      \"currency\": \"USD\",
      \"external_reference_id\": \"${USER_ID}\"
    }
  }")

ACCOUNT2_ID=$(echo "$OPEN2_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
if [[ -z "$ACCOUNT2_ID" ]]; then
  echo -e "${RED}Failed to open 2nd account. Skipping transfer demo.${NC}"
else
  success "2nd Account opened: $ACCOUNT2_ID"
  pause 2

  info "Approving 2nd account..."
  curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
    -H "$AUTH" -H "$CT" \
    -d "{\"ApproveAccount\":{\"id\":\"${ACCOUNT2_ID}\"}}" | python3 -m json.tool 2>/dev/null

  ACCOUNT2_VIEW=$(curl -s "${GATEWAY_URL}/v1/bank_account/${ACCOUNT2_ID}" -H "$AUTH")
  LEDGER2_ID=$(echo "$ACCOUNT2_VIEW" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  pause 2

  info "Depositing 500 USD into 2nd account..."
  curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
    -H "$AUTH" -H "$CT" \
    -d "{
      \"Deposit\": {
        \"id\": \"${ACCOUNT2_ID}\",
        \"amount\": {\"amount\": \"500\", \"currency\": \"USD\"}
      }
    }" | python3 -m json.tool 2>/dev/null

  info "Waiting for outbox (${OUTBOX_WAIT}s)..."
  pause "$OUTBOX_WAIT"
fi

# ------------------------------------------------------------------
# STEP 14: Transfer Between Accounts
# ------------------------------------------------------------------
step "14. Transfer 200 USD (Account 1 → Account 2)"

if [[ -n "$ACCOUNT2_ID" ]]; then
  TRANSFER_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
    -H "$AUTH" -H "$CT" \
    -d "{
      \"Transfer\": {
        \"id\": \"${ACCOUNT_ID}\",
        \"to_account_id\": \"${ACCOUNT2_ID}\",
        \"amount\": {\"amount\": \"200\", \"currency\": \"USD\"}
      }
    }")

  echo "$TRANSFER_RESULT" | python3 -m json.tool 2>/dev/null || echo "$TRANSFER_RESULT"
  success "Transfer submitted."

  info "Waiting for outbox (${OUTBOX_WAIT}s)..."
  pause "$OUTBOX_WAIT"

  info "Ledger 1 (should show 550 available: 1000 - 250 - 200):"
  curl -s "${GATEWAY_URL}/v1/ledger/${LEDGER_ID}" -H "$AUTH" | python3 -m json.tool 2>/dev/null

  if [[ -n "$LEDGER2_ID" ]]; then
    info "Ledger 2 (should show 700 available: 500 + 200):"
    curl -s "${GATEWAY_URL}/v1/ledger/${LEDGER2_ID}" -H "$AUTH" | python3 -m json.tool 2>/dev/null
  fi
else
  info "Skipped — 2nd account not created."
fi

# ------------------------------------------------------------------
# STEP 15: Open Sub-Account (Interest)
# ------------------------------------------------------------------
step "15. Open Interest Sub-Account (linked to Account 1)"

SUB_RESULT=$(curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
  -H "$AUTH" -H "$CT" \
  -d "{
    \"OpenAccount\": {
      \"account_type\": \"Retail\",
      \"kind\": \"Interest\",
      \"currency\": \"USD\",
      \"external_reference_id\": \"${USER_ID}\",
      \"parent_id\": \"${ACCOUNT_ID}\"
    }
  }")

echo "$SUB_RESULT" | python3 -m json.tool 2>/dev/null || echo "$SUB_RESULT"
SUB_ACCOUNT_ID=$(echo "$SUB_RESULT" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")

if [[ -n "$SUB_ACCOUNT_ID" ]]; then
  success "Sub-account created: $SUB_ACCOUNT_ID"
  pause 2

  info "Approving sub-account..."
  curl -s -X POST "${GATEWAY_URL}/v1/bank_account" \
    -H "$AUTH" -H "$CT" \
    -d "{\"ApproveAccount\":{\"id\":\"${SUB_ACCOUNT_ID}\"}}" | python3 -m json.tool 2>/dev/null
else
  echo -e "${RED}Failed to create sub-account.${NC}"
fi

# ------------------------------------------------------------------
# STEP 16: Query Sub-Accounts
# ------------------------------------------------------------------
step "16. List Sub-Accounts for Account 1"

curl -s "${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}/sub-accounts" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 17: Lookup by Account Number
# ------------------------------------------------------------------
step "17. Lookup Account by Number"

if [[ -n "$ACCOUNT_NUMBER" ]]; then
  info "Looking up account number: $ACCOUNT_NUMBER"
  curl -s "${GATEWAY_URL}/v1/bank_account/by-number/${ACCOUNT_NUMBER}" \
    -H "$AUTH" | python3 -m json.tool 2>/dev/null
else
  info "No account number captured, skipping."
fi

# ------------------------------------------------------------------
# STEP 18: Filtered Transactions (deposits only)
# ------------------------------------------------------------------
step "18. List Transactions (filtered: deposits only)"

curl -s "${GATEWAY_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID}&offset=0&limit=10&transaction_type=deposit" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 19: Paginated Accounts List
# ------------------------------------------------------------------
step "19. List All Accounts (paginated)"

curl -s "${GATEWAY_URL}/v1/accounts?offset=0&limit=5" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 20: Settlement Report (CSV)
# ------------------------------------------------------------------
step "20. Settlement Report (CSV)"

TODAY=$(date -u +%Y-%m-%d)
info "Date range: ${TODAY} to ${TODAY}"
curl -s "${GATEWAY_URL}/v1/report/settlement?start_date=${TODAY}&end_date=${TODAY}&currency=USD" \
  -H "$AUTH"
echo ""

# ------------------------------------------------------------------
# STEP 21: Balance History
# ------------------------------------------------------------------
step "21. Balance History (from daily snapshots)"

info "Note: Snapshots are created by the midnight UTC cron job."
curl -s "${GATEWAY_URL}/v1/bank_account/${ACCOUNT_ID}/balance-history?start_date=2026-01-01&end_date=2026-12-31" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# DONE
# ------------------------------------------------------------------
echo ""
echo -e "${GREEN}============================================================${NC}"
echo -e "${GREEN}  DEMO COMPLETE${NC}"
echo -e "${GREEN}============================================================${NC}"
echo ""
echo "Summary of IDs:"
echo "  User ID:          $USER_ID"
echo "  Bank Account ID:  $ACCOUNT_ID"
echo "  Account Number:   ${ACCOUNT_NUMBER:-N/A}"
echo "  Ledger ID:        ${LEDGER_ID:-N/A}"
echo "  2nd Account ID:   ${ACCOUNT2_ID:-N/A}"
echo "  Sub-Account ID:   ${SUB_ACCOUNT_ID:-N/A}"
echo "  House Acct (USD): ${HOUSE_USD_ID:-N/A}"
echo "  API Key:          ${API_KEY:0:20}..."
echo ""
echo "All API calls were routed through the Gateway (${GATEWAY_URL})."
echo "Check portal API logs to see the activity."
echo ""
echo "Cleanup: make docker-down (or make local-stop)"
