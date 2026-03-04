#!/usr/bin/env bash
#
# Bankie Core API Test Suite
# Direct Core API testing with JWT auth to :3030.
# Tests all Core endpoints: health, house accounts, bank account lifecycle,
# ledger, transactions, user view, sub-accounts, account number lookup,
# settlement report, balance history, freeze/unfreeze, close, and negatives.
#
# Prerequisites:
#   make local-setup   (starts infra + DB + server)
#   make local-jwt     (generates JWT token)
#
# Usage:
#   ./scripts/core-test.sh <JWT_TOKEN>
#   TOKEN=eyJ... ./scripts/core-test.sh
#
# Environment variables:
#   BASE_URL      - Core server URL (default: http://localhost:3030)
#   OUTBOX_WAIT   - Seconds to wait for outbox processing (default: 15)
#   TOKEN         - JWT token (alternative to argument)
#
set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
BASE_URL="${BASE_URL:-http://localhost:3030}"
TOKEN="${1:-${TOKEN:-}}"
OUTBOX_WAIT="${OUTBOX_WAIT:-15}"

if [[ -z "$TOKEN" ]]; then
  echo "ERROR: JWT token required."
  echo ""
  echo "Usage: ./scripts/core-test.sh <JWT_TOKEN>"
  echo "   or: TOKEN=eyJ... ./scripts/core-test.sh"
  echo ""
  echo "Generate a token with: make local-jwt SERVICE=core-test"
  exit 1
fi

AUTH="Authorization: Bearer ${TOKEN}"
CT="Content-Type: application/json"

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ---------------------------------------------------------------------------
# Test Framework
# ---------------------------------------------------------------------------
TOTAL=0
PASSED=0
FAILED=0
FAILED_TESTS=""

suite() {
  echo ""
  echo -e "${BLUE}============================================================${NC}"
  echo -e "${BLUE}  SUITE: $1${NC}"
  echo -e "${BLUE}============================================================${NC}"
}

run_test() {
  local name="$1"
  TOTAL=$((TOTAL + 1))
  echo -e "${CYAN}  TEST: ${name}${NC}"
}

pass() {
  PASSED=$((PASSED + 1))
  echo -e "    ${GREEN}PASS${NC}"
}

fail() {
  local reason="${1:-assertion failed}"
  FAILED=$((FAILED + 1))
  FAILED_TESTS="${FAILED_TESTS}\n    - ${reason}"
  echo -e "    ${RED}FAIL: ${reason}${NC}"
}

assert_status() {
  local actual="$1"
  local expected="$2"
  local context="${3:-}"
  if [[ "$actual" == "$expected" ]]; then
    return 0
  else
    fail "${context}: expected status ${expected}, got ${actual}"
    return 1
  fi
}

assert_json_field() {
  local json="$1"
  local field="$2"
  local expected="${3:-}"
  local context="${4:-}"
  local actual
  actual=$(echo "$json" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    keys = '${field}'.split('.')
    val = data
    for k in keys:
        if isinstance(val, list):
            val = val[int(k)]
        else:
            val = val[k]
    print(val)
except (KeyError, IndexError, TypeError):
    sys.exit(1)
" 2>/dev/null) || {
    fail "${context}: field '${field}' not found in response"
    return 1
  }

  if [[ -n "$expected" ]]; then
    if [[ "$actual" == "$expected" ]]; then
      return 0
    else
      fail "${context}: field '${field}' expected '${expected}', got '${actual}'"
      return 1
    fi
  fi
  return 0
}

assert_entries_count() {
  local json="$1"
  local min_count="$2"
  local context="${3:-}"
  local count
  count=$(echo "$json" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(len(data.get('entries', [])))
except:
    print(0)
" 2>/dev/null)
  if [[ "$count" -ge "$min_count" ]]; then
    return 0
  else
    fail "${context}: expected >= ${min_count} entries, got ${count}"
    return 1
  fi
}

# ---------------------------------------------------------------------------
# HTTP Helpers
# ---------------------------------------------------------------------------
HTTP_BODY=""
HTTP_STATUS=""

http_get() {
  local url="$1"
  local use_auth="${2:-yes}"
  local response
  if [[ "$use_auth" == "no" ]]; then
    response=$(curl -s -w "\n%{http_code}" "$url")
  else
    response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
  fi
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_post() {
  local url="$1"
  local data="$2"
  local use_auth="${3:-yes}"
  local response
  if [[ "$use_auth" == "no" ]]; then
    response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$CT" -d "$data")
  else
    response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$AUTH" -H "$CT" -d "$data")
  fi
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

wait_for_outbox() {
  local seconds="${1:-$OUTBOX_WAIT}"
  echo -e "    ${YELLOW}(waiting ${seconds}s for outbox processing...)${NC}"
  sleep "$seconds"
}

# ---------------------------------------------------------------------------
# State variables
# ---------------------------------------------------------------------------
HOUSE_USD_ID=""
ACCOUNT_ID_1=""
ACCOUNT_NUM_1=""
LEDGER_ID_1=""
ACCOUNT_ID_2=""
LEDGER_ID_2=""
SUB_ACCOUNT_ID=""
USER_ID="core-test-user-$(date +%s)"

echo ""
echo -e "${BLUE}╔══════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║       Bankie Core API Test Suite                 ║${NC}"
echo -e "${BLUE}║       Direct to Core :3030 with JWT              ║${NC}"
echo -e "${BLUE}╚══════════════════════════════════════════════════╝${NC}"
echo -e "  Target: ${BASE_URL}"
echo -e "  Token:  ${TOKEN:0:30}..."
echo ""

# ===========================================================================
# SUITE 1: Health Checks
# ===========================================================================
suite "1. Health Checks"

run_test "GET /health returns 200"
http_get "${BASE_URL}/health" "no"
if assert_status "$HTTP_STATUS" "200" "/health"; then
  pass
fi

run_test "GET /ready returns 200 with status=ready"
http_get "${BASE_URL}/ready" "no"
if assert_status "$HTTP_STATUS" "200" "/ready" && \
   assert_json_field "$HTTP_BODY" "status" "ready" "/ready body"; then
  pass
fi

# ===========================================================================
# SUITE 2: House Account Management
# ===========================================================================
suite "2. House Account Management"

run_test "POST /v1/house_account -- create USD house account"
http_post "${BASE_URL}/v1/house_account" '{
  "status": "active",
  "account_name": "Core Test USD Settlement",
  "account_type": "Settlement",
  "currency": "USD"
}'
if assert_status "$HTTP_STATUS" "201" "create house USD"; then
  HOUSE_USD_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$HOUSE_USD_ID" ]]; then
    pass
  else
    fail "create house USD: could not extract id"
  fi
fi

run_test "GET /v1/house_account?currency=USD -- list USD house accounts"
http_get "${BASE_URL}/v1/house_account?currency=USD"
if assert_status "$HTTP_STATUS" "200" "list house USD" && \
   assert_entries_count "$HTTP_BODY" 1 "list house USD"; then
  pass
fi

# ===========================================================================
# SUITE 3: Account Lifecycle
# ===========================================================================
suite "3. Account Lifecycle (Open → Approve → Query)"

run_test "POST /v1/bank_account -- OpenAccount (USD Checking)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open account"; then
  ACCOUNT_ID_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  ACCOUNT_NUM_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['account_number'])" 2>/dev/null || echo "")
  if [[ -n "$ACCOUNT_ID_1" && -n "$ACCOUNT_NUM_1" ]]; then
    pass
  else
    fail "open account: could not extract id/account_number"
  fi
fi

sleep 2

run_test "GET /v1/bank_account/:id -- query account (should be Pending)"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query pending" && \
   assert_json_field "$HTTP_BODY" "status" "Pending" "account status"; then
  pass
fi

run_test "POST /v1/bank_account -- ApproveAccount (creates ledger)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${ACCOUNT_ID_1}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve account"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- account is Approved with ledger_id"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query approved"; then
  LEDGER_ID_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Approved" && -n "$LEDGER_ID_1" ]]; then
    pass
  else
    fail "not approved or missing ledger_id (status=${local_status})"
  fi
fi

# ===========================================================================
# SUITE 4: Deposit + Ledger
# ===========================================================================
suite "4. Deposit + Ledger Verification"

run_test "POST /v1/bank_account -- Deposit 1000 USD"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {\"amount\": \"1000\", \"currency\": \"USD\"}
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit 1000 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- balance should be 1000"
http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
if assert_status "$HTTP_STATUS" "200" "ledger after deposit"; then
  available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${available}') == 1000.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "ledger available expected 1000, got ${available}"
  fi
fi

# ===========================================================================
# SUITE 5: Withdrawal + Ledger
# ===========================================================================
suite "5. Withdrawal + Ledger Verification"

run_test "POST /v1/bank_account -- Withdraw 250 USD"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {\"amount\": \"250\", \"currency\": \"USD\"}
  }
}"
if assert_status "$HTTP_STATUS" "200" "withdraw 250 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- balance should be 750"
http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
if assert_status "$HTTP_STATUS" "200" "ledger after withdrawal"; then
  available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${available}') == 750.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "ledger available expected 750, got ${available}"
  fi
fi

# ===========================================================================
# SUITE 6: Account 2 + Transfer
# ===========================================================================
suite "6. Account 2 + Transfer"

run_test "Open + Approve Account 2 (USD Checking)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open account 2"; then
  ACCOUNT_ID_2=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$ACCOUNT_ID_2" ]]; then
    pass
  else
    fail "open account 2: could not extract id"
  fi
fi

sleep 2

run_test "POST /v1/bank_account -- ApproveAccount 2"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {\"id\": \"${ACCOUNT_ID_2}\"}
}"
if assert_status "$HTTP_STATUS" "200" "approve account 2"; then
  pass
fi

sleep 2

http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
LEDGER_ID_2=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")

run_test "POST /v1/bank_account -- Transfer 200 USD (acct 1 → acct 2)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Transfer\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"to_account_id\": \"${ACCOUNT_ID_2}\",
    \"amount\": {\"amount\": \"200\", \"currency\": \"USD\"}
  }
}"
if assert_status "$HTTP_STATUS" "200" "transfer 200 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger -- acct 1 balance should be 550"
http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
if assert_status "$HTTP_STATUS" "200" "ledger 1 after transfer"; then
  available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${available}') == 550.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "ledger 1 available expected 550, got ${available}"
  fi
fi

run_test "GET /v1/ledger -- acct 2 balance should be 200"
http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_2}"
if assert_status "$HTTP_STATUS" "200" "ledger 2 after transfer"; then
  available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${available}') == 200.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "ledger 2 available expected 200, got ${available}"
  fi
fi

# ===========================================================================
# SUITE 7: Query Endpoints
# ===========================================================================
suite "7. Query Endpoints"

run_test "GET /v1/transaction -- list transactions for account 1"
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10"
if assert_status "$HTTP_STATUS" "200" "list transactions" && \
   assert_entries_count "$HTTP_BODY" 1 "transactions count"; then
  pass
fi

run_test "GET /v1/transaction -- filter deposits only"
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10&transaction_type=deposit"
if assert_status "$HTTP_STATUS" "200" "list deposit transactions" && \
   assert_entries_count "$HTTP_BODY" 1 "deposit transactions count"; then
  pass
fi

run_test "GET /v1/user/:id -- user view"
http_get "${BASE_URL}/v1/user/${USER_ID}"
if assert_status "$HTTP_STATUS" "200" "user view" && \
   assert_entries_count "$HTTP_BODY" 1 "user accounts"; then
  pass
fi

run_test "GET /v1/bank_account/by-number/:num -- lookup by account number"
http_get "${BASE_URL}/v1/bank_account/by-number/${ACCOUNT_NUM_1}"
if assert_status "$HTTP_STATUS" "200" "lookup by number" && \
   assert_json_field "$HTTP_BODY" "id" "$ACCOUNT_ID_1" "account id match"; then
  pass
fi

run_test "GET /v1/accounts -- paginated list"
http_get "${BASE_URL}/v1/accounts?offset=0&limit=5"
if assert_status "$HTTP_STATUS" "200" "list accounts" && \
   assert_entries_count "$HTTP_BODY" 1 "accounts list"; then
  pass
fi

# ===========================================================================
# SUITE 8: Sub-Account
# ===========================================================================
suite "8. Sub-Account"

run_test "POST /v1/bank_account -- open Interest sub-account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Interest\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${USER_ID}\",
    \"parent_id\": \"${ACCOUNT_ID_1}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open sub-account"; then
  SUB_ACCOUNT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_ACCOUNT_ID" ]]; then
    pass
  else
    fail "open sub-account: could not extract id"
  fi
fi

sleep 2

run_test "POST /v1/bank_account -- approve sub-account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {\"id\": \"${SUB_ACCOUNT_ID}\"}
}"
if assert_status "$HTTP_STATUS" "200" "approve sub-account"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id/sub-accounts -- list sub-accounts"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}/sub-accounts"
if assert_status "$HTTP_STATUS" "200" "list sub-accounts" && \
   assert_entries_count "$HTTP_BODY" 1 "sub-account count"; then
  pass
fi

# ===========================================================================
# SUITE 9: Reports
# ===========================================================================
suite "9. Reports"

TODAY=$(date -u +%Y-%m-%d)

run_test "GET /v1/report/settlement -- CSV settlement report"
http_get "${BASE_URL}/v1/report/settlement?start_date=${TODAY}&end_date=${TODAY}&currency=USD"
if assert_status "$HTTP_STATUS" "200" "settlement report"; then
  pass
fi

run_test "GET /v1/bank_account/:id/balance-history -- balance history"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}/balance-history?start_date=2026-01-01&end_date=2026-12-31"
if assert_status "$HTTP_STATUS" "200" "balance history"; then
  pass
fi

# ===========================================================================
# SUITE 10: Freeze / Unfreeze / Close
# ===========================================================================
suite "10. Freeze, Unfreeze, Close"

run_test "POST /v1/bank_account -- FreezeAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"FreezeAccount\": {\"id\": \"${ACCOUNT_ID_2}\"}
}"
if assert_status "$HTTP_STATUS" "200" "freeze account"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify Frozen status"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query frozen" && \
   assert_json_field "$HTTP_BODY" "status" "Frozen" "frozen status"; then
  pass
fi

run_test "POST /v1/bank_account -- UnfreezeAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"UnfreezeAccount\": {\"id\": \"${ACCOUNT_ID_2}\"}
}"
if assert_status "$HTTP_STATUS" "200" "unfreeze account"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify Approved status after unfreeze"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query unfrozen" && \
   assert_json_field "$HTTP_BODY" "status" "Approved" "unfrozen status"; then
  pass
fi

run_test "POST /v1/bank_account -- CloseAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"CloseAccount\": {\"id\": \"${ACCOUNT_ID_2}\"}
}"
if assert_status "$HTTP_STATUS" "200" "close account"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify CustomerClosed status"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query closed" && \
   assert_json_field "$HTTP_BODY" "status" "CustomerClosed" "closed status"; then
  pass
fi

# ===========================================================================
# SUITE 11: Negative Cases
# ===========================================================================
suite "11. Negative Cases"

run_test "GET /v1/bank_account/nonexistent -- 404"
http_get "${BASE_URL}/v1/bank_account/00000000-0000-0000-0000-000000000000"
if assert_status "$HTTP_STATUS" "404" "nonexistent account"; then
  pass
fi

run_test "POST /v1/bank_account -- deposit to closed account -- should fail"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${ACCOUNT_ID_2}\",
    \"amount\": {\"amount\": \"100\", \"currency\": \"USD\"}
  }
}"
if [[ "$HTTP_STATUS" =~ ^4 ]]; then
  pass
else
  fail "deposit to closed: expected 4xx, got ${HTTP_STATUS}"
fi

run_test "POST /v1/bank_account -- withdraw more than balance -- should fail"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {\"amount\": \"999999\", \"currency\": \"USD\"}
  }
}"
if [[ "$HTTP_STATUS" =~ ^4 ]]; then
  pass
else
  fail "over-withdraw: expected 4xx, got ${HTTP_STATUS}"
fi

run_test "GET /v1/bank_account/by-number/FAKE000000 -- not found"
http_get "${BASE_URL}/v1/bank_account/by-number/FAKE000000"
if assert_status "$HTTP_STATUS" "404" "fake account number"; then
  pass
fi

# ===========================================================================
# RESULTS
# ===========================================================================
echo ""
echo -e "${BLUE}============================================================${NC}"
echo -e "${BLUE}  RESULTS${NC}"
echo -e "${BLUE}============================================================${NC}"
echo ""
echo -e "  Total:  ${TOTAL}"
echo -e "  Passed: ${GREEN}${PASSED}${NC}"
echo -e "  Failed: ${RED}${FAILED}${NC}"

if [[ $FAILED -gt 0 ]]; then
  echo -e "\n  Failed tests:${FAILED_TESTS}"
  echo ""
  exit 1
fi

echo ""
echo -e "${GREEN}All ${TOTAL} tests passed!${NC}"
echo ""
