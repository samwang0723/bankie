#!/usr/bin/env bash
#
# Bankie E2E Test Suite
# Comprehensive end-to-end testing for all API endpoints.
#
# Tests the full account lifecycle: health checks, house account management,
# account opening/approval, deposits, withdrawals, transfers, freeze/unfreeze,
# close, query endpoints, negative cases, and sub-account scenarios
# (master/interest/yield with parent_id linkage and cross-account transfers).
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

run_test "GET /v1/bank_account/:id -- verify external_reference_id on pending account"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query ext_ref on pending account"; then
  ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
  if [[ "$ext_ref" == "$USER_ID" ]]; then
    pass
  else
    fail "external_reference_id expected '${USER_ID}', got '${ext_ref}'"
  fi
fi

run_test "GET /v1/bank_account/:id -- verify kind=Checking on account 1"
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}"
if assert_status "$HTTP_STATUS" "200" "query kind on account 1"; then
  kind_val=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('kind',''))" 2>/dev/null || echo "")
  if [[ "$kind_val" == "Checking" ]]; then
    pass
  else
    fail "kind expected 'Checking', got '${kind_val}'"
  fi
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

run_test "GET /v1/ledger/:id -- verify book_balance after deposit"
if [[ -n "$LEDGER_ID_1" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
  if assert_status "$HTTP_STATUS" "200" "ledger book_balance after deposit"; then
    book_balance=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['book_balance']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${book_balance}') == 1000.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger book_balance expected 1000, got ${book_balance}"
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

run_test "GET /v1/ledger/:id -- verify book_balance after withdrawal"
if [[ -n "$LEDGER_ID_1" ]]; then
  http_get "${BASE_URL}/v1/ledger/${LEDGER_ID_1}"
  if assert_status "$HTTP_STATUS" "200" "ledger book_balance after withdrawal"; then
    book_balance=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['book_balance']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${book_balance}') == 750.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "ledger book_balance expected 750, got ${book_balance}"
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

run_test "GET /v1/user/:id -- verify entries contain book_balance field"
http_get "${BASE_URL}/v1/user/${USER_ID}"
if assert_status "$HTTP_STATUS" "200" "user query book_balance"; then
  has_book_balance=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
all_have = all('book_balance' in e for e in entries if e.get('status') == 'Approved')
print('true' if all_have and len(entries) > 0 else 'false')
" 2>/dev/null || echo "false")
  if [[ "$has_book_balance" == "true" ]]; then
    pass
  else
    fail "user entries missing book_balance field"
  fi
fi

run_test "GET /v1/user/:id -- verify account 1 available balance in user view"
http_get "${BASE_URL}/v1/user/${USER_ID}"
if assert_status "$HTTP_STATUS" "200" "user query account 1 balance"; then
  acct1_available=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
for e in data.get('entries', []):
    if e.get('id') == '${ACCOUNT_ID_1}':
        print(e.get('available', ''))
        sys.exit(0)
print('')
" 2>/dev/null || echo "")
  if python3 -c "exit(0 if float('${acct1_available}') == 650.0 else 1)" 2>/dev/null; then
    pass
  else
    fail "user view account 1 available expected 650, got ${acct1_available}"
  fi
fi

run_test "GET /v1/user/:id -- nonexistent user returns empty entries"
http_get "${BASE_URL}/v1/user/nonexistent-user-id-00000"
if assert_status "$HTTP_STATUS" "200" "nonexistent user query"; then
  count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries', [])))" 2>/dev/null || echo "-1")
  if [[ "$count" == "0" ]]; then
    pass
  else
    fail "nonexistent user expected 0 entries, got ${count}"
  fi
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

run_test "GET /v1/transaction -- verify transaction_type field present"
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10"
if assert_status "$HTTP_STATUS" "200" "transaction type field"; then
  has_type=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
all_have = all('transaction_type' in e for e in entries)
print('true' if all_have and len(entries) > 0 else 'false')
" 2>/dev/null || echo "false")
  if [[ "$has_type" == "true" ]]; then
    pass
  else
    fail "transaction entries missing transaction_type field"
  fi
fi

run_test "GET /v1/transaction -- filter by transaction_type=deposit"
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10&transaction_type=deposit"
if assert_status "$HTTP_STATUS" "200" "transaction filter by type"; then
  all_deposits=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
all_match = all(e.get('transaction_type') == 'deposit' for e in entries)
print('true' if all_match and len(entries) >= 1 else 'false')
" 2>/dev/null || echo "false")
  if [[ "$all_deposits" == "true" ]]; then
    pass
  else
    fail "filtered transactions should all be deposits"
  fi
fi

run_test "GET /v1/transaction -- filter by date range"
TODAY=$(date +%Y-%m-%d)
YESTERDAY=$(date -v-1d +%Y-%m-%d 2>/dev/null || date -d "yesterday" +%Y-%m-%d 2>/dev/null || echo "$TODAY")
http_get "${BASE_URL}/v1/transaction?bank_account_id=${ACCOUNT_ID_1}&offset=0&limit=10&start_date=${YESTERDAY}&end_date=${TODAY}"
if assert_status "$HTTP_STATUS" "200" "transaction filter by date"; then
  has_pagination=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
print('true' if 'pagination' in data else 'false')
" 2>/dev/null || echo "false")
  if [[ "$has_pagination" == "true" ]]; then
    pass
  else
    fail "filtered transaction query should include pagination"
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

run_test "GET /v1/bank_account/by-number/:num -- verify returned fields match"
if [[ -n "$ACCOUNT_NUM_1" ]]; then
  http_get "${BASE_URL}/v1/bank_account/by-number/${ACCOUNT_NUM_1}"
  if assert_status "$HTTP_STATUS" "200" "by-number field check"; then
    returned_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || echo "")
    if [[ "$returned_id" == "$ACCOUNT_ID_1" ]]; then
      pass
    else
      fail "by-number lookup returned id '${returned_id}', expected '${ACCOUNT_ID_1}'"
    fi
  fi
else
  fail "no account_number captured"
fi

run_test "GET /v1/accounts -- paginated account list"
http_get "${BASE_URL}/v1/accounts?offset=0&limit=10"
if assert_status "$HTTP_STATUS" "200" "accounts list"; then
  has_pagination=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
p = data.get('pagination', {})
has_fields = 'total' in p and 'offset' in p and 'limit' in p
has_entries = len(data.get('entries', [])) >= 1
print('true' if has_fields and has_entries else 'false')
" 2>/dev/null || echo "false")
  if [[ "$has_pagination" == "true" ]]; then
    pass
  else
    fail "accounts list missing pagination fields or entries"
  fi
fi

run_test "GET /v1/accounts -- verify pagination offset/limit"
http_get "${BASE_URL}/v1/accounts?offset=0&limit=1"
if assert_status "$HTTP_STATUS" "200" "accounts list limit=1"; then
  entry_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('entries', [])))" 2>/dev/null || echo "0")
  limit_val=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('pagination',{}).get('limit',0))" 2>/dev/null || echo "0")
  if [[ "$entry_count" -le 1 && "$limit_val" == "1" ]]; then
    pass
  else
    fail "accounts list limit=1: entries=${entry_count}, pagination.limit=${limit_val}"
  fi
fi

run_test "GET /v1/bank_account/:id/balance-history -- balance history"
TODAY=$(date +%Y-%m-%d)
YESTERDAY=$(date -v-1d +%Y-%m-%d 2>/dev/null || date -d "yesterday" +%Y-%m-%d 2>/dev/null || echo "$TODAY")
http_get "${BASE_URL}/v1/bank_account/${ACCOUNT_ID_1}/balance-history?start_date=${YESTERDAY}&end_date=${TODAY}"
if assert_status "$HTTP_STATUS" "200" "balance history"; then
  pass
fi

run_test "GET /v1/report/settlement -- settlement report CSV"
TODAY=$(date +%Y-%m-%d)
YESTERDAY=$(date -v-1d +%Y-%m-%d 2>/dev/null || date -d "yesterday" +%Y-%m-%d 2>/dev/null || echo "$TODAY")
http_get "${BASE_URL}/v1/report/settlement?start_date=${YESTERDAY}&end_date=${TODAY}"
if assert_status "$HTTP_STATUS" "200" "settlement report"; then
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

# NOTE: Business rule violations (overdraft, frozen ops, duplicate close, etc.)
# return 200 at HTTP level because CQRS commands are accepted into the async
# processing channel. Errors surface during aggregate processing, not at HTTP layer.
# Only input validation (zero amount, invalid JSON, unsupported currency) returns 400.

# ===========================================================================
# SUITE 10: Sub-Account Scenario
# ===========================================================================
suite "10. Sub-Account Scenario (Master + Interest + Yield)"

SUB_USER_ID="e2e-sub-user-$(date +%s)"
SUB_MASTER_ID=""
SUB_MASTER_LEDGER_ID=""
SUB_INTEREST_ID=""
SUB_INTEREST_LEDGER_ID=""
SUB_YIELD_ID=""
SUB_YIELD_LEDGER_ID=""

# --- Step 1: Open master Checking account ---
run_test "POST /v1/bank_account -- OpenAccount master (USD Checking) for sub-account user"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"user_id\": \"${SUB_USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open sub-account master"; then
  SUB_MASTER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_MASTER_ID" ]]; then
    pass
  else
    fail "open sub-account master: could not extract id"
  fi
fi

sleep 2

# --- Step 2: Approve master account ---
run_test "POST /v1/bank_account -- ApproveAccount master"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${SUB_MASTER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve sub-account master"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- get master ledger_id"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}"
if assert_status "$HTTP_STATUS" "200" "query sub-account master"; then
  SUB_MASTER_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Approved" && -n "$SUB_MASTER_LEDGER_ID" ]]; then
    pass
  else
    fail "master not approved or missing ledger_id (status=${local_status}, ledger=${SUB_MASTER_LEDGER_ID})"
  fi
fi

# --- Step 3: Open Interest sub-account (auto-resolves parent_id to master) ---
run_test "POST /v1/bank_account -- OpenAccount Interest sub-account (USD)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Interest\",
    \"currency\": \"USD\",
    \"user_id\": \"${SUB_USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open interest sub-account"; then
  SUB_INTEREST_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_INTEREST_ID" ]]; then
    pass
  else
    fail "open interest sub-account: could not extract id"
  fi
fi

sleep 2

# --- Step 4: Open Yield sub-account (auto-resolves parent_id to master) ---
run_test "POST /v1/bank_account -- OpenAccount Yield sub-account (USD)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Yield\",
    \"currency\": \"USD\",
    \"user_id\": \"${SUB_USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open yield sub-account"; then
  SUB_YIELD_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_YIELD_ID" ]]; then
    pass
  else
    fail "open yield sub-account: could not extract id"
  fi
fi

sleep 2

# --- Step 5: Approve Interest + Yield sub-accounts ---
run_test "POST /v1/bank_account -- ApproveAccount Interest"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${SUB_INTEREST_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve interest sub-account"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- get Interest ledger_id"
http_get "${BASE_URL}/v1/bank_account/${SUB_INTEREST_ID}"
if assert_status "$HTTP_STATUS" "200" "query interest sub-account"; then
  SUB_INTEREST_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_INTEREST_LEDGER_ID" ]]; then
    pass
  else
    fail "interest sub-account missing ledger_id"
  fi
fi

run_test "POST /v1/bank_account -- ApproveAccount Yield"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${SUB_YIELD_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve yield sub-account"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- get Yield ledger_id"
http_get "${BASE_URL}/v1/bank_account/${SUB_YIELD_ID}"
if assert_status "$HTTP_STATUS" "200" "query yield sub-account"; then
  SUB_YIELD_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  if [[ -n "$SUB_YIELD_LEDGER_ID" ]]; then
    pass
  else
    fail "yield sub-account missing ledger_id"
  fi
fi

# --- Step 6: Verify parent_id linkage via sub-accounts endpoint ---
run_test "GET /v1/bank_account/:id/sub-accounts -- verify master + sub-accounts structure"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}/sub-accounts"
if assert_status "$HTTP_STATUS" "200" "sub-accounts query"; then
  # Verify master exists
  master_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['master']['id'])" 2>/dev/null || echo "")
  sub_count=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin)['sub_accounts']))" 2>/dev/null || echo "0")
  if [[ "$master_id" == "$SUB_MASTER_ID" && "$sub_count" -ge 2 ]]; then
    pass
  else
    fail "sub-accounts: expected master=${SUB_MASTER_ID} with >= 2 subs, got master=${master_id} subs=${sub_count}"
  fi
fi

run_test "GET /v1/bank_account/:id/sub-accounts -- verify parent_id on sub-accounts"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}/sub-accounts"
if assert_status "$HTTP_STATUS" "200" "sub-accounts parent_id check"; then
  # Check that all sub-accounts have parent_id matching master
  parent_ids_valid=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
subs = data.get('sub_accounts', [])
master_id = '${SUB_MASTER_ID}'
all_valid = all(s.get('parent_id') == master_id for s in subs)
print('true' if all_valid and len(subs) >= 2 else 'false')
" 2>/dev/null || echo "false")
  if [[ "$parent_ids_valid" == "true" ]]; then
    pass
  else
    fail "sub-accounts parent_id mismatch -- expected all sub_accounts.parent_id == ${SUB_MASTER_ID}"
  fi
fi

# --- Step 7: Deposit 500 USD into master ---
run_test "POST /v1/bank_account -- Deposit 500 USD into master"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${SUB_MASTER_ID}\",
    \"amount\": {
      \"amount\": \"500\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit 500 to master"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- verify master ledger = 500"
if [[ -n "$SUB_MASTER_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_MASTER_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "master ledger after deposit"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 500.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "master ledger available expected 500, got ${available}"
    fi
  fi
else
  fail "no master ledger_id to query"
fi

# --- Step 8: Transfer 200 USD from master to Interest sub-account ---
run_test "POST /v1/bank_account -- Transfer 200 USD from master to Interest"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Transfer\": {
    \"id\": \"${SUB_MASTER_ID}\",
    \"to_account_id\": \"${SUB_INTEREST_ID}\",
    \"amount\": {
      \"amount\": \"200\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "transfer 200 master->interest"; then
  pass
fi

wait_for_outbox

# --- Step 9: Verify ledger balances after transfer ---
run_test "GET /v1/ledger/:id -- verify master ledger = 300 after transfer"
if [[ -n "$SUB_MASTER_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_MASTER_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "master ledger after transfer"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 300.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "master ledger available expected 300, got ${available}"
    fi
  fi
else
  fail "no master ledger_id to query"
fi

run_test "GET /v1/ledger/:id -- verify Interest ledger = 200 after transfer"
if [[ -n "$SUB_INTEREST_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_INTEREST_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "interest ledger after transfer"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 200.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "interest ledger available expected 200, got ${available}"
    fi
  fi
else
  fail "no interest ledger_id to query"
fi

run_test "GET /v1/ledger/:id -- verify Yield ledger = 0 (untouched)"
if [[ -n "$SUB_YIELD_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_YIELD_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "yield ledger untouched"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 0.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "yield ledger available expected 0, got ${available}"
    fi
  fi
else
  fail "no yield ledger_id to query"
fi

# --- Step 10: Deposit directly into Yield sub-account ---
run_test "POST /v1/bank_account -- Deposit 100 USD directly into Yield sub-account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${SUB_YIELD_ID}\",
    \"amount\": {
      \"amount\": \"100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit 100 to yield"; then
  pass
fi

wait_for_outbox

run_test "GET /v1/ledger/:id -- verify Yield ledger = 100 after direct deposit"
if [[ -n "$SUB_YIELD_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_YIELD_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "yield ledger after deposit"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 100.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "yield ledger available expected 100, got ${available}"
    fi
  fi
else
  fail "no yield ledger_id to query"
fi

# --- Step 11: Query user view -- verify all 3 sub-account-user accounts ---
run_test "GET /v1/user/:id -- verify sub-account user has 3 accounts"
http_get "${BASE_URL}/v1/user/${SUB_USER_ID}"
if assert_status "$HTTP_STATUS" "200" "sub-account user query" && \
   assert_entries_count "$HTTP_BODY" 3 "sub-account user accounts (expected 3)"; then
  pass
fi

# --- Step 12: Verify kind on sub-accounts ---
run_test "GET /v1/bank_account/:id -- verify master kind=Checking"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}"
if assert_status "$HTTP_STATUS" "200" "master kind check"; then
  kind_val=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('kind',''))" 2>/dev/null || echo "")
  if [[ "$kind_val" == "Checking" ]]; then
    pass
  else
    fail "master kind expected 'Checking', got '${kind_val}'"
  fi
fi

run_test "GET /v1/bank_account/:id -- verify Interest kind=Interest"
http_get "${BASE_URL}/v1/bank_account/${SUB_INTEREST_ID}"
if assert_status "$HTTP_STATUS" "200" "interest kind check"; then
  kind_val=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('kind',''))" 2>/dev/null || echo "")
  if [[ "$kind_val" == "Interest" ]]; then
    pass
  else
    fail "interest kind expected 'Interest', got '${kind_val}'"
  fi
fi

run_test "GET /v1/bank_account/:id -- verify Yield kind=Yield"
http_get "${BASE_URL}/v1/bank_account/${SUB_YIELD_ID}"
if assert_status "$HTTP_STATUS" "200" "yield kind check"; then
  kind_val=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('kind',''))" 2>/dev/null || echo "")
  if [[ "$kind_val" == "Yield" ]]; then
    pass
  else
    fail "yield kind expected 'Yield', got '${kind_val}'"
  fi
fi

# --- Step 13: Verify external_reference_id on all sub-account views ---
run_test "GET /v1/bank_account/:id -- verify external_reference_id on master"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}"
if assert_status "$HTTP_STATUS" "200" "master ext_ref check"; then
  ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
  if [[ "$ext_ref" == "$SUB_USER_ID" ]]; then
    pass
  else
    fail "master external_reference_id expected '${SUB_USER_ID}', got '${ext_ref}'"
  fi
fi

run_test "GET /v1/bank_account/:id -- verify external_reference_id on Interest sub"
http_get "${BASE_URL}/v1/bank_account/${SUB_INTEREST_ID}"
if assert_status "$HTTP_STATUS" "200" "interest ext_ref check"; then
  ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
  if [[ "$ext_ref" == "$SUB_USER_ID" ]]; then
    pass
  else
    fail "interest external_reference_id expected '${SUB_USER_ID}', got '${ext_ref}'"
  fi
fi

run_test "GET /v1/bank_account/:id -- verify external_reference_id on Yield sub"
http_get "${BASE_URL}/v1/bank_account/${SUB_YIELD_ID}"
if assert_status "$HTTP_STATUS" "200" "yield ext_ref check"; then
  ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
  if [[ "$ext_ref" == "$SUB_USER_ID" ]]; then
    pass
  else
    fail "yield external_reference_id expected '${SUB_USER_ID}', got '${ext_ref}'"
  fi
fi

# --- Step 14: Verify sub-accounts kinds in sub-accounts endpoint ---
run_test "GET /v1/bank_account/:id/sub-accounts -- verify sub-account kinds"
http_get "${BASE_URL}/v1/bank_account/${SUB_MASTER_ID}/sub-accounts"
if assert_status "$HTTP_STATUS" "200" "sub-accounts kinds check"; then
  kinds_valid=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
subs = data.get('sub_accounts', [])
kinds = set(s.get('kind') for s in subs)
expected = {'Interest', 'Yield'}
print('true' if kinds == expected else 'false')
" 2>/dev/null || echo "false")
  if [[ "$kinds_valid" == "true" ]]; then
    pass
  else
    fail "sub-accounts should have kinds {Interest, Yield}"
  fi
fi

# --- Step 15: Verify book_balance on sub-account ledgers ---
run_test "GET /v1/ledger/:id -- verify Interest book_balance = 200"
if [[ -n "$SUB_INTEREST_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_INTEREST_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "interest book_balance check"; then
    book_balance=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['book_balance']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${book_balance}') == 200.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "interest book_balance expected 200, got ${book_balance}"
    fi
  fi
else
  fail "no interest ledger_id to query"
fi

run_test "GET /v1/ledger/:id -- verify Yield book_balance = 100"
if [[ -n "$SUB_YIELD_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${SUB_YIELD_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "yield book_balance check"; then
    book_balance=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['book_balance']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${book_balance}') == 100.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "yield book_balance expected 100, got ${book_balance}"
    fi
  fi
else
  fail "no yield ledger_id to query"
fi

# ===========================================================================
# SUITE 11: external_reference_id Comprehensive Testing
# ===========================================================================
suite "11. external_reference_id Comprehensive Testing"

EXT_REF_USER="e2e-extref-user-$(date +%s)"
EXTREF_ACCT_ID=""
EXTREF_ACCT_NUM=""

run_test "POST /v1/bank_account -- OpenAccount with external_reference_id field"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"external_reference_id\": \"${EXT_REF_USER}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open account with external_reference_id"; then
  EXTREF_ACCT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  EXTREF_ACCT_NUM=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['account_number'])" 2>/dev/null || echo "")
  if [[ -n "$EXTREF_ACCT_ID" ]]; then
    pass
  else
    fail "could not extract id from external_reference_id account"
  fi
fi

sleep 2

run_test "GET /v1/bank_account/:id -- verify external_reference_id set correctly"
if [[ -n "$EXTREF_ACCT_ID" ]]; then
  http_get "${BASE_URL}/v1/bank_account/${EXTREF_ACCT_ID}"
  if assert_status "$HTTP_STATUS" "200" "query extref account"; then
    ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
    if [[ "$ext_ref" == "$EXT_REF_USER" ]]; then
      pass
    else
      fail "external_reference_id expected '${EXT_REF_USER}', got '${ext_ref}'"
    fi
  fi
else
  fail "no extref account id"
fi

run_test "POST /v1/bank_account -- ApproveAccount for extref account"
if [[ -n "$EXTREF_ACCT_ID" ]]; then
  http_post "${BASE_URL}/v1/bank_account" "{
    \"ApproveAccount\": {
      \"id\": \"${EXTREF_ACCT_ID}\"
    }
  }"
  if assert_status "$HTTP_STATUS" "200" "approve extref account"; then
    pass
  fi
else
  fail "no extref account id"
fi

sleep 2

run_test "GET /v1/user/:id -- query by external_reference_id returns the account"
http_get "${BASE_URL}/v1/user/${EXT_REF_USER}"
if assert_status "$HTTP_STATUS" "200" "user query by ext_ref"; then
  found=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
ids = [e.get('id') for e in entries]
print('true' if '${EXTREF_ACCT_ID}' in ids else 'false')
" 2>/dev/null || echo "false")
  if [[ "$found" == "true" ]]; then
    pass
  else
    fail "user query by ext_ref did not return account ${EXTREF_ACCT_ID}"
  fi
fi

run_test "GET /v1/user/:id -- verify external_reference_id survives approval"
if [[ -n "$EXTREF_ACCT_ID" ]]; then
  http_get "${BASE_URL}/v1/bank_account/${EXTREF_ACCT_ID}"
  if assert_status "$HTTP_STATUS" "200" "extref after approval"; then
    ext_ref=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('external_reference_id',''))" 2>/dev/null || echo "")
    acct_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || echo "")
    if [[ "$ext_ref" == "$EXT_REF_USER" && "$acct_status" == "Approved" ]]; then
      pass
    else
      fail "extref after approval: ext_ref='${ext_ref}', status='${acct_status}'"
    fi
  fi
else
  fail "no extref account id"
fi

run_test "POST /v1/bank_account -- OpenAccount with no external_reference_id"
http_post "${BASE_URL}/v1/bank_account" '{
  "OpenAccount": {
    "account_type": "Retail",
    "kind": "Checking",
    "currency": "USD"
  }
}'
if assert_status "$HTTP_STATUS" "201" "open account without ext_ref"; then
  NO_EXTREF_ACCT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$NO_EXTREF_ACCT_ID" ]]; then
    pass
  else
    fail "could not extract id from no-extref account"
  fi
fi

sleep 2

run_test "GET /v1/bank_account/:id -- verify external_reference_id is null when not set"
if [[ -n "$NO_EXTREF_ACCT_ID" ]]; then
  http_get "${BASE_URL}/v1/bank_account/${NO_EXTREF_ACCT_ID}"
  if assert_status "$HTTP_STATUS" "200" "query no-extref account"; then
    ext_ref_check=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
ref = data.get('external_reference_id')
print('null' if ref is None or ref == '' else ref)
" 2>/dev/null || echo "")
    if [[ "$ext_ref_check" == "null" || "$ext_ref_check" == "" ]]; then
      pass
    else
      fail "expected null/empty external_reference_id, got '${ext_ref_check}'"
    fi
  fi
else
  fail "no no-extref account id"
fi

# NOTE: Duplicate account creation (same ext_ref + currency + kind) is not
# rejected at HTTP level -- OpenAccount always returns 201 (CQRS async).

# ===========================================================================
# SUITE 12: Frozen Account Operations
# ===========================================================================
suite "12. Frozen Account Operations"

FREEZE_USER_ID="e2e-freeze-user-$(date +%s)"
FREEZE_ACCT_ID=""
FREEZE_LEDGER_ID=""

run_test "POST /v1/bank_account -- OpenAccount for freeze tests"
http_post "${BASE_URL}/v1/bank_account" "{
  \"OpenAccount\": {
    \"account_type\": \"Retail\",
    \"kind\": \"Checking\",
    \"currency\": \"USD\",
    \"user_id\": \"${FREEZE_USER_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "201" "open freeze-test account"; then
  FREEZE_ACCT_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])" 2>/dev/null || echo "")
  if [[ -n "$FREEZE_ACCT_ID" ]]; then
    pass
  else
    fail "could not extract freeze-test account id"
  fi
fi

sleep 2

run_test "POST /v1/bank_account -- Approve freeze-test account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"ApproveAccount\": {
    \"id\": \"${FREEZE_ACCT_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "approve freeze-test account"; then
  pass
fi

sleep 2

run_test "GET /v1/bank_account/:id -- get freeze-test ledger_id"
http_get "${BASE_URL}/v1/bank_account/${FREEZE_ACCT_ID}"
if assert_status "$HTTP_STATUS" "200" "query freeze-test account"; then
  FREEZE_LEDGER_ID=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['ledger_id'])" 2>/dev/null || echo "")
  if [[ -n "$FREEZE_LEDGER_ID" ]]; then
    pass
  else
    fail "freeze-test account missing ledger_id"
  fi
fi

run_test "POST /v1/bank_account -- Deposit 500 USD into freeze-test account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${FREEZE_ACCT_ID}\",
    \"amount\": {
      \"amount\": \"500\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit to freeze-test account"; then
  pass
fi

wait_for_outbox

run_test "POST /v1/bank_account -- Freeze the account"
http_post "${BASE_URL}/v1/bank_account" "{
  \"FreezeAccount\": {
    \"id\": \"${FREEZE_ACCT_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "freeze account"; then
  pass
fi

sleep 1

# NOTE: CQRS async — commands to frozen accounts are accepted (200) at HTTP level
# but fail during aggregate processing. We verify the ledger stays unchanged below.
run_test "POST /v1/bank_account -- Deposit to frozen account -- accepted (async fail)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Deposit\": {
    \"id\": \"${FREEZE_ACCT_ID}\",
    \"amount\": {
      \"amount\": \"100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "deposit to frozen accepted"; then
  pass
fi

run_test "POST /v1/bank_account -- Withdrawal from frozen account -- accepted (async fail)"
http_post "${BASE_URL}/v1/bank_account" "{
  \"Withdrawal\": {
    \"id\": \"${FREEZE_ACCT_ID}\",
    \"amount\": {
      \"amount\": \"100\",
      \"currency\": \"USD\"
    }
  }
}"
if assert_status "$HTTP_STATUS" "200" "withdrawal from frozen accepted"; then
  pass
fi

run_test "GET /v1/ledger/:id -- verify frozen account ledger unchanged"
if [[ -n "$FREEZE_LEDGER_ID" ]]; then
  http_get "${BASE_URL}/v1/ledger/${FREEZE_LEDGER_ID}"
  if assert_status "$HTTP_STATUS" "200" "frozen ledger check"; then
    available=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['available']['amount'])" 2>/dev/null || echo "")
    if python3 -c "exit(0 if float('${available}') == 500.0 else 1)" 2>/dev/null; then
      pass
    else
      fail "frozen ledger available expected 500, got ${available}"
    fi
  fi
else
  fail "no freeze ledger_id to query"
fi

run_test "POST /v1/bank_account -- Unfreeze account for cleanup"
http_post "${BASE_URL}/v1/bank_account" "{
  \"UnfreezeAccount\": {
    \"id\": \"${FREEZE_ACCT_ID}\"
  }
}"
if assert_status "$HTTP_STATUS" "200" "unfreeze for cleanup"; then
  pass
fi

sleep 1

run_test "GET /v1/bank_account/:id -- verify account is Approved after unfreeze"
http_get "${BASE_URL}/v1/bank_account/${FREEZE_ACCT_ID}"
if assert_status "$HTTP_STATUS" "200" "query unfrozen freeze-test account"; then
  local_status=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['status'])" 2>/dev/null || echo "")
  if [[ "$local_status" == "Approved" ]]; then
    pass
  else
    fail "freeze-test account expected Approved, got ${local_status}"
  fi
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
