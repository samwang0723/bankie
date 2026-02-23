#!/usr/bin/env bash
#
# Bankie E2E Test Suite
# Comprehensive end-to-end testing for all API endpoints.
#
# Tests the full account lifecycle: health checks, house account management,
# account opening/approval, deposits, withdrawals, transfers, freeze/unfreeze,
# close, query endpoints, and negative cases.
#
# Prerequisites:
#   make local-setup   (starts infra + DB + server)
#   make local-jwt     (generates JWT token)
#
# Usage:
#   ./scripts/e2e-test.sh <JWT_TOKEN>
#
# Or set the TOKEN env var:
#   TOKEN=eyJ... ./scripts/e2e-test.sh
#
# Environment variables:
#   BASE_URL      - Server URL (default: http://localhost:3030)
#   OUTBOX_WAIT   - Seconds to wait for async outbox processing (default: 15)
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
  echo "Usage: ./scripts/e2e-test.sh <JWT_TOKEN>"
  echo "   or: TOKEN=eyJ... ./scripts/e2e-test.sh"
  echo ""
  echo "Generate a token with: make local-jwt SERVICE=demo-service"
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

# Assert HTTP status code
# Usage: assert_status <actual_status> <expected_status> <test_context>
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

# Assert JSON field exists and optionally matches expected value
# Usage: assert_json_field <json> <field> [expected_value] [test_context]
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

# Assert JSON response has entries array with at least N items
# Usage: assert_entries_count <json> <min_count> [test_context]
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

# HTTP helper: captures both body and status code
# Usage: http_get <url> [auth]
#   Sets HTTP_BODY and HTTP_STATUS
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

# Usage: http_post <url> <data> [auth]
#   Sets HTTP_BODY and HTTP_STATUS
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
  echo -e "    ${YELLOW}(waiting ${seconds}s for outbox cron processing...)${NC}"
  sleep "$seconds"
}

# ---------------------------------------------------------------------------
# State variables (populated during tests)
# ---------------------------------------------------------------------------
HOUSE_USD_ID=""
HOUSE_TWD_ID=""
ACCOUNT_ID_1=""
ACCOUNT_NUM_1=""
LEDGER_ID_1=""
ACCOUNT_ID_2=""
LEDGER_ID_2=""
USER_ID="e2e-test-user-$(date +%s)"

# ===========================================================================
# SUITE 1: Health Checks (no auth)
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
  "account_name": "E2E Test USD Settlement",
  "account_type": "House",
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

run_test "POST /v1/house_account -- create TWD house account"
http_post "${BASE_URL}/v1/house_account" '{
  "status": "active",
  "account_name": "E2E Test TWD Settlement",
  "account_type": "House",
  "currency": "TWD"
}'
if assert_status "$HTTP_STATUS" "201" "create house TWD"; then
  HOUSE_TWD_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$HOUSE_TWD_ID" ]]; then
    pass
  else
    fail "create house TWD: could not extract id"
  fi
fi

run_test "GET /v1/house_account?currency=USD -- list USD house accounts"
http_get "${BASE_URL}/v1/house_account?currency=USD"
if assert_status "$HTTP_STATUS" "200" "list house USD" && \
   assert_entries_count "$HTTP_BODY" 1 "list house USD"; then
  pass
fi

run_test "GET /v1/house_account?currency=TWD -- list TWD house accounts"
http_get "${BASE_URL}/v1/house_account?currency=TWD"
if assert_status "$HTTP_STATUS" "200" "list house TWD" && \
   assert_entries_count "$HTTP_BODY" 1 "list house TWD"; then
  pass
fi

# ===========================================================================
# SUITE 3: Account Lifecycle -- Account 1
# ===========================================================================
suite "3. Account Lifecycle (Account 1 - USD Checking)"

run_test "POST /v1/bank_account -- OpenAccount (USD Checking)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"user_id\": \"${USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open account 1"; then
  ACCOUNT_ID_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  ACCOUNT_NUM_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['account_number'])" 2>/dev/null || echo "")
  if [[ -n "$ACCOUNT_ID_1" && -n "$ACCOUNT_NUM_1" ]]; then
    pass
  else
    fail "open account 1: could not extract id/account_number"
  fi
fi

sleep 2

run_test "GET /v1/bank_account/:id -- query account (should be Pending)"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query pending account" && \
   assert_json_field "$HTTP_BODY" "status" "Pending" "account status"; then
  pass
fi

run_test "POST /v1/bank_account -- ApproveAccount (creates ledger)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${ACCOUNT_ID_1}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve account 1"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- query account (should be Approved with ledger_id)"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query approved account"; then
  LEDGER_ID_1=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Approved" && -n "$LEDGER_ID_1" ]]; then
    pass
  else
    fail "account 1 not approved or missing ledger_id (status=${local_status}, ledger=${LEDGER_ID_1})"
  fi
fi

# ===========================================================================
# SUITE 4: Deposit + Ledger Verification
# ===========================================================================
suite "4. Deposit + Ledger Verification"

run_test "POST /v1/bank_account -- Deposit 1000 USD"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {
      \"amount\": \"1000\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit 1000 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- verify ledger balance after deposit"
if [[ -n "$LEDGER_ID_1" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
  if assert_status "$HTTP_STATUS" "200" "ledger after deposit"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 1000.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger available expected 1000, got ${available}"
    fi
  fi
else
  fail "no ledger_id to query"
fi

# ===========================================================================
# SUITE 5: Withdrawal + Ledger Verification
# ===========================================================================
suite "5. Withdrawal + Ledger Verification"

run_test "POST /v1/bank_account -- Withdraw 250 USD"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {
      \"amount\": \"250\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "withdraw 250 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- verify ledger balance after withdrawal"
if [[ -n "$LEDGER_ID_1" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
  if assert_status "$HTTP_STATUS" "200" "ledger after withdrawal"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 750.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger available expected 750, got ${available}"
    fi
  fi
else
  fail "no ledger_id to query"
fi

# ===========================================================================
# SUITE 6: Account 2 + Transfer
# ===========================================================================
suite "6. Account 2 + Transfer"

run_test "POST /v1/bank_account -- OpenAccount 2 (USD Savings)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Interest\",
    \"currency\": \"USD\",
    \"user_id\": \"${USER_ID}\"
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
  \"ApproveAccount\": {
    \"id\": \"${ACCOUNT_ID_2}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve account 2"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- get account 2 ledger_id"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query account 2"; then
  LEDGER_ID_2=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  if [[ -n "$LEDGER_ID_2" ]]; then
    pass
  else
    fail "account 2 missing ledger_id"
  fi
fi

run_test "POST /v1/bank_account -- Transfer 100 USD from account 1 to account 2"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Transfer\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"to_account_id\": \"${ACCOUNT_ID_2}\",
    \"amount\": {
      \"amount\": \"100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "transfer 100 USD"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- verify account 1 ledger after transfer"
if [[ -n "$LEDGER_ID_1" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
  if assert_status "$HTTP_STATUS" "200" "ledger 1 after transfer"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 650.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger 1 available expected 650, got ${available}"
    fi
  fi
else
  fail "no ledger_id_1 to query"
fi

run_test "GET /v1/ledger/:id -- verify account 2 ledger after transfer"
if [[ -n "$LEDGER_ID_2" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_2}"
  if assert_status "$HTTP_STATUS" "200" "ledger 2 after transfer"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 100.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger 2 available expected 100, got ${available}"
    fi
  fi
else
  fail "no ledger_id_2 to query"
fi

# ===========================================================================
# SUITE 7: Freeze / Unfreeze / Close
# ===========================================================================
suite "7. Account State Management (Freeze / Unfreeze / Close)"

run_test "POST /v1/bank_account -- FreezeAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"FreezeAccount\": {
    \"id\": \"${ACCOUNT_ID_2}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "freeze account 2" && \
   assert_json_field "$HTTP_BODY" "status" "frozen" "freeze response"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify account is Freeze"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query frozen account"; then
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Freeze" ]]; then
    pass
  else
    fail "account 2 expected Freeze, got ${local_status}"
  fi
fi

run_test "POST /v1/bank_account -- UnfreezeAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"UnfreezeAccount\": {
    \"id\": \"${ACCOUNT_ID_2}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "unfreeze account 2" && \
   assert_json_field "$HTTP_BODY" "status" "unfrozen" "unfreeze response"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify account is Approved after unfreeze"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query unfrozen account"; then
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Approved" ]]; then
    pass
  else
    fail "account 2 expected Approved after unfreeze, got ${local_status}"
  fi
fi

run_test "POST /v1/bank_account -- Withdraw remaining balance from account 2 before close"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${ACCOUNT_ID_2}\",
    \"amount\": {
      \"amount\": \"100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "withdraw remaining balance from account 2"; then
  pass
fi

wait_for_outbox

run_test "POST /v1/bank_account -- CloseAccount"
http_post "${BASE_URL}/v1/bank_account" "{
  \"CloseAccount\": {
    \"id\": \"${ACCOUNT_ID_2}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "close account 2" && \
   assert_json_field "$HTTP_BODY" "status" "closed" "close response"; then
  pass
fi

sleep 3

run_test "GET /v1/bank_account/:id -- verify account is CustomerClosed"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_2}"
if assert_status "$HTTP_STATUS" "200" "query closed account"; then
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "CustomerClosed" ]]; then
    pass
  else
    fail "account 2 expected CustomerClosed, got ${local_status}"
  fi
fi

# ===========================================================================
# SUITE 8: Query Endpoints
# ===========================================================================
suite "8. Query Endpoints"

run_test "GET /v1/user/:id -- list user's bank accounts"
http_get "${BASE_URL}/v1/user/${USER_ID}"
if assert_status "$HTTP_STATUS" "200" "user query" && \
   assert_entries_count "$HTTP_BODY" 2 "user accounts (expected 2)"; then
  pass
fi

run_test "GET /v1/transaction -- list transactions for account 1"
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10"
if assert_status "$HTTP_STATUS" "200" "transaction list"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries', [])))" 2>/dev/null || echo "0")
  if [[ "$count" -ge 1 ]]; then
    pass
  else
    fail "transaction list: expected >= 1 transactions, got ${count}"
  fi
fi

run_test "GET /v1/bank_account/:id/sub-accounts -- sub-accounts query"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}/sub-accounts"
if assert_status "$HTTP_STATUS" "200" "sub-accounts query"; then
  pass
fi

run_test "GET /v1/bank_account/by-number/:num -- lookup by account number"
if [[ -n "$ACCOUNT_NUM_1" ]]; then
  http_get "${BASE_URL}/v1/bank_account/by-number/${ACCOUNT_NUM_1}"
  if assert_status "$HTTP_STATUS" "200" "by-number lookup"; then
    pass
  fi
else
  fail "no account_number captured"
fi

run_test "GET /v1/bank_account/:id/balance-history -- balance history"
TODAY=$(date +%Y-%m-%d)
YESTERDAY=$(date -v-1d +%Y-%m-%d 2>/dev/null || date -d "yesterday" +%Y-%m-%d 2>/dev/null || echo "$TODAY")
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}/balance-history?start_date=${YESTERDAY}&end_date=${TODAY}"
if assert_status "$HTTP_STATUS" "200" "balance history"; then
  pass
fi

# ===========================================================================
# SUITE 9: Negative Cases
# ===========================================================================
suite "9. Negative Cases"

run_test "GET /v1/bank_account/:id without auth -- expect 403"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}" "no"
if assert_status "$HTTP_STATUS" "403" "no auth 403"; then
  pass
fi

run_test "GET /v1/bank_account/:id with invalid ID -- expect 404"
http_get "${BASE_URL}/v1/bank_account/00000000-0000-0000-0000-000000000000"
if assert_status "$HTTP_STATUS" "404" "not found 404"; then
  pass
fi

run_test "POST /v1/bank_account with invalid body -- expect 400"
http_post "${BASE_URL}/v1/bank_account" '{"invalid": "body"}'
if assert_status "$HTTP_STATUS" "400" "bad request 400"; then
  pass
fi

run_test "POST /v1/bank_account -- Deposit with zero amount -- expect 400"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {
      \"amount\": \"0\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "400" "zero deposit 400"; then
  pass
fi

run_test "POST /v1/bank_account -- Withdrawal with negative amount -- expect 400"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"amount\": {
      \"amount\": \"-100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "400" "negative withdrawal 400"; then
  pass
fi

run_test "POST /v1/bank_account -- Transfer with zero amount -- expect 400"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Transfer\": {
    \"id\": \"${ACCOUNT_ID_1}\",
    \"to_account_id\": \"${ACCOUNT_ID_2}\",
    \"amount\": {
      \"amount\": \"0\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "400" "zero transfer 400"; then
  pass
fi

run_test "POST /v1/house_account -- unsupported currency -- expect 400"
http_post "${BASE_URL}/v1/house_account" '{
  "status": "active",
  "account_name": "Bad Currency",
  "account_type": "Settlement",
  "currency": "FAKECOIN"
}'
if assert_status "$HTTP_STATUS" "400" "unsupported currency 400"; then
  pass
fi

# ===========================================================================
# REPORT
# ===========================================================================
echo ""
echo -e "${BLUE}============================================================${NC}"
echo -e "${BLUE}  E2E TEST REPORT${NC}"
echo -e "${BLUE}============================================================${NC}"
echo ""
echo -e "  Total:   ${TOTAL}"
echo -e "  ${GREEN}Passed:  ${PASSED}${NC}"
echo -e "  ${RED}Failed:  ${FAILED}${NC}"

if [[ $FAILED -gt 0 ]]; then
  echo ""
  echo -e "${RED}Failed tests:${FAILED_TESTS}${NC}"
  echo ""
  echo -e "${RED}E2E TESTS FAILED${NC}"
  exit 1
else
  echo ""
  echo -e "${GREEN}ALL E2E TESTS PASSED${NC}"
  exit 0
fi
