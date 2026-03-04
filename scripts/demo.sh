#!/usr/bin/env bash
#
# Bankie Demo Script
# Walks through: house account creation, account opening, KYC approval,
# deposit, withdrawal, and reporting queries.
#
# Prerequisites:
#   make local-setup   (starts infra + DB + server)
#   make local-jwt     (generates JWT token)
#
# Usage:
#   ./scripts/demo.sh <JWT_TOKEN>
#
# Or set the TOKEN env var:
#   TOKEN=eyJ... ./scripts/demo.sh
#
set -euo pipefail

BASE_URL="${BASE_URL:-http://localhost:3030}"
TOKEN="${1:-${TOKEN:-}}"

if [[ -z "$TOKEN" ]]; then
  echo "ERROR: JWT token required."
  echo ""
  echo "Usage: ./scripts/demo.sh <JWT_TOKEN>"
  echo "   or: TOKEN=eyJ... ./scripts/demo.sh"
  echo ""
  echo "Generate a token with: make local-jwt SERVICE=demo-service"
  exit 1
fi

AUTH="Authorization: Bearer ${TOKEN}"
CT="Content-Type: application/json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

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
# STEP 1: Create House Accounts (one per currency)
# House accounts are the bank's own settlement accounts (double-entry counterparty).
# ------------------------------------------------------------------
step "1. Create House Account (USD)"

HOUSE_USD=$(curl -s -X POST "${BASE_URL}/v1/house_account" \
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

HOUSE_TWD=$(curl -s -X POST "${BASE_URL}/v1/house_account" \
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
curl -s "${BASE_URL}/v1/house_account?currency=USD" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

info "TWD house accounts:"
curl -s "${BASE_URL}/v1/house_account?currency=TWD" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 3: Open a Bank Account
# Server auto-generates the account ID.
# ------------------------------------------------------------------
step "3. Open Bank Account (USD, Retail/Checking)"

USER_ID="aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"

OPEN_RESULT=$(curl -s -X POST "${BASE_URL}/v1/bank_account" \
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

if [[ -z "$ACCOUNT_ID" ]]; then
  echo -e "${RED}Failed to open account. Exiting.${NC}"
  exit 1
fi
success "Bank Account opened: $ACCOUNT_ID"

pause 2

# ------------------------------------------------------------------
# STEP 4: Query Bank Account (should be Pending)
# ------------------------------------------------------------------
step "4. Query Bank Account (status should be Pending)"

curl -s "${BASE_URL}/v1/bank_account/${ACCOUNT_ID}" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 5: Approve Account (KYC)
# This creates the ledger for the account.
# ------------------------------------------------------------------
step "5. Approve Account (KYC) -- creates ledger"

APPROVE_RESULT=$(curl -s -X POST "${BASE_URL}/v1/bank_account" \
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

ACCOUNT_VIEW=$(curl -s "${BASE_URL}/v1/bank_account/${ACCOUNT_ID}" -H "$AUTH")
echo "$ACCOUNT_VIEW" | python3 -m json.tool 2>/dev/null
LEDGER_ID=$(echo "$ACCOUNT_VIEW" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")

if [[ -n "$LEDGER_ID" ]]; then
  success "Ledger ID: $LEDGER_ID"
fi

# ------------------------------------------------------------------
# STEP 7: Deposit
# ------------------------------------------------------------------
step "7. Deposit 1000 USD"

DEPOSIT_RESULT=$(curl -s -X POST "${BASE_URL}/v1/bank_account" \
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

info "Waiting for outbox cron to process ledger credit (up to 15s)..."
pause 15

# ------------------------------------------------------------------
# STEP 8: Check Ledger After Deposit
# ------------------------------------------------------------------
step "8. Query Ledger (should show 1000 available)"

if [[ -n "$LEDGER_ID" ]]; then
  curl -s "${BASE_URL}/v1/ledger/${LEDGER_ID}" \
    -H "$AUTH" | python3 -m json.tool 2>/dev/null
else
  info "No ledger_id captured, skipping."
fi

# ------------------------------------------------------------------
# STEP 9: Withdrawal
# ------------------------------------------------------------------
step "9. Withdraw 250 USD"

WITHDRAW_RESULT=$(curl -s -X POST "${BASE_URL}/v1/bank_account" \
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

info "Waiting for outbox cron to process debit-release (up to 15s)..."
pause 15

# ------------------------------------------------------------------
# STEP 10: Check Ledger After Withdrawal
# ------------------------------------------------------------------
step "10. Query Ledger (should show 750 available)"

if [[ -n "$LEDGER_ID" ]]; then
  curl -s "${BASE_URL}/v1/ledger/${LEDGER_ID}" \
    -H "$AUTH" | python3 -m json.tool 2>/dev/null
else
  info "No ledger_id captured, skipping."
fi

# ------------------------------------------------------------------
# STEP 11: Transactions List
# ------------------------------------------------------------------
step "11. List Transactions"

curl -s "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID}&offset=0&limit=10" \
  -H "$AUTH" | python3 -m json.tool 2>/dev/null

# ------------------------------------------------------------------
# STEP 12: User View (all accounts + ledgers)
# ------------------------------------------------------------------
step "12. User View (all bank accounts for this user)"

curl -s "${BASE_URL}/v1/user/${USER_ID}" \
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
echo "  Ledger ID:        ${LEDGER_ID:-N/A}"
echo "  House Acct (USD): ${HOUSE_USD_ID:-N/A}"
echo ""
echo "Cleanup: make local-stop"
