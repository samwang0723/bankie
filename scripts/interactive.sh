#!/usr/bin/env bash
#
# Bankie Interactive Console
# Menu-driven tool for testing all Bankie API endpoints.
#
# Core API operations (1-20) route through the Gateway (:4040) using API key auth,
# ensuring all calls appear in portal API logs.
# Portal operations (21-35) use cookie-based session auth on the Gateway.
#
# Usage:
#   ./scripts/interactive.sh                        # interactive setup
#   API_KEY=bk_live_... ./scripts/interactive.sh    # use existing key
#
set -uo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
GATEWAY_URL="${GATEWAY_URL:-http://localhost:4040}"
OUTBOX_WAIT="${OUTBOX_WAIT:-15}"
API_KEY="${API_KEY:-}"

# ---------------------------------------------------------------------------
# Colors
# ---------------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

# ---------------------------------------------------------------------------
# State — remembers last-used values
# ---------------------------------------------------------------------------
LAST_ACCOUNT_ID=""
LAST_LEDGER_ID=""
LAST_USER_ID=""
LAST_ACCOUNT_NUMBER=""
LAST_CURRENCY="USD"
LAST_RATE_CONFIG_ID=""
PORTAL_LOGGED_IN=""
PORTAL_EMAIL=""

# ---------------------------------------------------------------------------
# API Key Resolution
# ---------------------------------------------------------------------------
if [[ -z "$API_KEY" ]]; then
  # Try .api-key file
  for f in .api-key .docker-api-key .local-api-key; do
    if [[ -f "$f" && -s "$f" ]]; then
      API_KEY=$(cat "$f")
      echo -e "${DIM}  Using API key from ${f}${NC}"
      break
    fi
  done
fi

# Core API auth header (set when API_KEY is available)
AUTH=""
CT="Content-Type: application/json"
if [[ -n "$API_KEY" ]]; then
  AUTH="Authorization: Bearer ${API_KEY}"
fi

# ---------------------------------------------------------------------------
# HTTP Helpers (Core API via Gateway — API key auth)
# ---------------------------------------------------------------------------
HTTP_BODY=""
HTTP_STATUS=""

api_key_check() {
  if [[ -z "$API_KEY" ]]; then
    echo -e "  ${RED}No API key set. Use option 21 (login) + 27 (create key) first,${NC}"
    echo -e "  ${RED}or set API_KEY env var.${NC}"
    return 1
  fi
  return 0
}

http_get() {
  local url="$1"
  local response
  response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_post() {
  local url="$1"
  local body="$2"
  local response
  response=$(curl -s -w "\n%{http_code}" -X POST "$url" -H "$AUTH" -H "$CT" -d "$body")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_put() {
  local url="$1"
  local body="$2"
  local response
  response=$(curl -s -w "\n%{http_code}" -X PUT "$url" -H "$AUTH" -H "$CT" -d "$body")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

http_get_raw() {
  local url="$1"
  curl -s -D /dev/stderr "$url" -H "$AUTH" 2>/tmp/bankie_headers
}

pretty_json() {
  echo "$1" | python3 -m json.tool 2>/dev/null || echo "$1"
}

print_status() {
  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}HTTP ${HTTP_STATUS}${NC}"
  else
    echo -e "  ${RED}HTTP ${HTTP_STATUS}${NC}"
  fi
}

# ---------------------------------------------------------------------------
# Prompt helpers — use last value as default
# ---------------------------------------------------------------------------
prompt() {
  local label="$1"
  local default="$2"
  local result
  if [[ -n "$default" ]]; then
    read -rp "  ${label} [${default}]: " result
    echo "${result:-$default}"
  else
    read -rp "  ${label}: " result
    echo "$result"
  fi
}

separator() {
  echo -e "${DIM}──────────────────────────────────────────────────────${NC}"
}

# ---------------------------------------------------------------------------
# Portal Session Helpers (cookie-based auth for Gateway Portal API)
# ---------------------------------------------------------------------------
COOKIE_JAR="/tmp/bankie_portal_cookies.txt"

portal_get_csrf() {
  grep 'csrf_token' "$COOKIE_JAR" 2>/dev/null | awk '{print $NF}' | tail -1
}

portal_check_session() {
  if [[ -z "$PORTAL_LOGGED_IN" ]]; then
    echo -e "  ${RED}Not logged in to portal. Use option 21 first.${NC}"
    return 1
  fi
  return 0
}

portal_get() {
  local url="$1"
  local response
  response=$(curl -s -w "\n%{http_code}" -b "$COOKIE_JAR" "$url")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

portal_post() {
  local url="$1"
  local body="${2:-{}}"
  local csrf
  csrf=$(portal_get_csrf)
  local response
  response=$(curl -s -w "\n%{http_code}" -X POST \
    -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H "$CT" -H "X-CSRF-Token: ${csrf}" \
    -d "$body" "$url")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

portal_put() {
  local url="$1"
  local body="${2:-{}}"
  local csrf
  csrf=$(portal_get_csrf)
  local response
  response=$(curl -s -w "\n%{http_code}" -X PUT \
    -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H "$CT" -H "X-CSRF-Token: ${csrf}" \
    -d "$body" "$url")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

portal_delete() {
  local url="$1"
  local csrf
  csrf=$(portal_get_csrf)
  local response
  response=$(curl -s -w "\n%{http_code}" -X DELETE \
    -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    -H "X-CSRF-Token: ${csrf}" \
    "$url")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
}

# ---------------------------------------------------------------------------
# Menu Actions — Core API (1-20) — via Gateway with API key
# ---------------------------------------------------------------------------

action_health() {
  echo -e "\n${CYAN}Health Check${NC}"
  separator

  echo -e "  ${DIM}/health${NC}"
  local r
  r=$(curl -s -w "\n%{http_code}" "${GATEWAY_URL}/health")
  local status=$(echo "$r" | tail -1)
  local body=$(echo "$r" | sed '$d')
  if [[ "$status" == "200" ]]; then
    echo -e "  ${GREEN}Healthy (${status})${NC}"
  else
    echo -e "  ${RED}Unhealthy (${status})${NC}  ${body}"
  fi

  echo -e "  ${DIM}/ready${NC}"
  r=$(curl -s -w "\n%{http_code}" "${GATEWAY_URL}/ready")
  status=$(echo "$r" | tail -1)
  body=$(echo "$r" | sed '$d')
  if [[ "$status" == "200" ]]; then
    echo -e "  ${GREEN}Ready (${status})${NC}  $(pretty_json "$body")"
  else
    echo -e "  ${RED}Not ready (${status})${NC}  ${body}"
  fi
}

action_list_house_accounts() {
  echo -e "\n${CYAN}List House Accounts${NC}"
  separator
  api_key_check || return
  local currency
  currency=$(prompt "Currency (USD/TWD/BTC/ETH/USDT or blank for all)" "")
  [[ -n "$currency" ]] && LAST_CURRENCY="$currency"

  local url="${GATEWAY_URL}/v1/house_account"
  [[ -n "$currency" ]] && url="${url}?currency=${currency}"

  http_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
}

action_create_house_account() {
  echo -e "\n${CYAN}Create House Account${NC}"
  separator
  api_key_check || return
  local currency name acct_type
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  name=$(prompt "Account name" "Master ${currency} Settlement")
  acct_type=$(prompt "Account type (Settlement/House)" "Settlement")

  local body
  body=$(cat <<ENDJSON
{
  "status": "active",
  "account_name": "${name}",
  "account_type": "${acct_type}",
  "currency": "${currency}"
}
ENDJSON
  )

  http_post "${GATEWAY_URL}/v1/house_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_CURRENCY="$currency"
}

action_open_account() {
  echo -e "\n${CYAN}Open Bank Account${NC}"
  separator
  api_key_check || return
  local acct_type kind currency ext_ref
  acct_type=$(prompt "Account type (Retail/Institution/Tax)" "Retail")
  kind=$(prompt "Kind (Checking/Interest/Yield)" "Checking")
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  ext_ref=$(prompt "External reference ID (user ID, optional)" "$LAST_USER_ID")

  local body
  body=$(cat <<ENDJSON
{
  "OpenAccount": {
    "account_type": "${acct_type}",
    "kind": "${kind}",
    "currency": "${currency}"$([ -n "$ext_ref" ] && echo ",
    \"external_reference_id\": \"${ext_ref}\"")
  }
}
ENDJSON
  )

  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"

  # Extract and remember IDs
  local new_id
  new_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  if [[ -n "$new_id" ]]; then
    LAST_ACCOUNT_ID="$new_id"
    echo -e "\n  ${GREEN}Account ID saved: ${new_id}${NC}"
  fi
  [[ -n "$ext_ref" ]] && LAST_USER_ID="$ext_ref"
  LAST_CURRENCY="$currency"
}

action_approve_account() {
  echo -e "\n${CYAN}Approve Account (KYC)${NC}"
  separator
  api_key_check || return
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  local body="{\"ApproveAccount\":{\"id\":\"${id}\"}}"

  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_deposit() {
  echo -e "\n${CYAN}Deposit${NC}"
  separator
  api_key_check || return
  local id amount currency
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  amount=$(prompt "Amount" "1000")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  local body
  body=$(cat <<ENDJSON
{
  "Deposit": {
    "id": "${id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_withdraw() {
  echo -e "\n${CYAN}Withdrawal${NC}"
  separator
  api_key_check || return
  local id amount currency
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  amount=$(prompt "Amount" "100")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  local body
  body=$(cat <<ENDJSON
{
  "Withdrawal": {
    "id": "${id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_transfer() {
  echo -e "\n${CYAN}Transfer${NC}"
  separator
  api_key_check || return
  local from_id to_id amount currency
  from_id=$(prompt "From account ID" "$LAST_ACCOUNT_ID")
  to_id=$(prompt "To account ID" "")
  amount=$(prompt "Amount" "100")
  currency=$(prompt "Currency" "$LAST_CURRENCY")

  if [[ -z "$to_id" ]]; then
    echo -e "  ${RED}Destination account ID is required.${NC}"
    return
  fi

  local body
  body=$(cat <<ENDJSON
{
  "Transfer": {
    "id": "${from_id}",
    "to_account_id": "${to_id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )

  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$from_id"
  LAST_CURRENCY="$currency"

  echo -e "\n  ${YELLOW}Note: Ledger update is async via outbox (${OUTBOX_WAIT}s).${NC}"
}

action_freeze_unfreeze() {
  echo -e "\n${CYAN}Freeze / Unfreeze Account${NC}"
  separator
  api_key_check || return
  local id choice
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  echo "  1) Freeze"
  echo "  2) Unfreeze"
  read -rp "  Choice [1]: " choice
  choice="${choice:-1}"

  local cmd
  if [[ "$choice" == "2" ]]; then
    cmd="UnfreezeAccount"
  else
    cmd="FreezeAccount"
  fi

  local body="{\"${cmd}\":{\"id\":\"${id}\"}}"
  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_close_account() {
  echo -e "\n${CYAN}Close Account${NC}"
  separator
  api_key_check || return
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  local body="{\"CloseAccount\":{\"id\":\"${id}\"}}"
  http_post "${GATEWAY_URL}/v1/bank_account" "$body"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_view_account() {
  echo -e "\n${CYAN}View Account Details${NC}"
  separator
  api_key_check || return
  local id
  id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")

  http_get "${GATEWAY_URL}/v1/bank_account/${id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"

  # Extract ledger_id for convenience
  local lid
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"
}

action_view_ledger() {
  echo -e "\n${CYAN}View Ledger Balance${NC}"
  separator
  api_key_check || return
  local id
  id=$(prompt "Ledger ID" "$LAST_LEDGER_ID")

  if [[ -z "$id" ]]; then
    echo -e "  ${YELLOW}Tip: View an account first (option 3) to auto-fill the ledger ID.${NC}"
    return
  fi

  http_get "${GATEWAY_URL}/v1/ledger/${id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_LEDGER_ID="$id"
}

action_list_user_accounts() {
  echo -e "\n${CYAN}List User Accounts${NC}"
  separator
  api_key_check || return
  local user_id
  user_id=$(prompt "User ID (external_reference_id)" "$LAST_USER_ID")

  if [[ -z "$user_id" ]]; then
    echo -e "  ${RED}User ID is required.${NC}"
    return
  fi

  http_get "${GATEWAY_URL}/v1/user/${user_id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_USER_ID="$user_id"

  # Extract first account ID if available
  local first_id
  first_id=$(echo "$HTTP_BODY" | python3 -c "
import sys,json
data=json.load(sys.stdin)
entries=data.get('entries',[])
if entries: print(entries[0].get('id',''))
else: print('')
" 2>/dev/null)
  [[ -n "$first_id" ]] && LAST_ACCOUNT_ID="$first_id"
}

action_lookup_by_number() {
  echo -e "\n${CYAN}Lookup Account by Number${NC}"
  separator
  api_key_check || return
  local num
  num=$(prompt "Account number" "$LAST_ACCOUNT_NUMBER")

  if [[ -z "$num" ]]; then
    echo -e "  ${RED}Account number is required.${NC}"
    return
  fi

  http_get "${GATEWAY_URL}/v1/bank_account/by-number/${num}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_NUMBER="$num"

  local aid lid
  aid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$aid" ]] && LAST_ACCOUNT_ID="$aid"
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"
}

action_sub_accounts() {
  echo -e "\n${CYAN}List Sub-Accounts${NC}"
  separator
  api_key_check || return
  local id
  id=$(prompt "Master account ID" "$LAST_ACCOUNT_ID")

  http_get "${GATEWAY_URL}/v1/bank_account/${id}/sub-accounts"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_transactions() {
  echo -e "\n${CYAN}List Transactions${NC}"
  separator
  api_key_check || return
  local id offset limit start_date end_date tx_type status
  id=$(prompt "Bank account ID" "$LAST_ACCOUNT_ID")
  offset=$(prompt "Offset" "0")
  limit=$(prompt "Limit" "20")
  start_date=$(prompt "Start date (YYYY-MM-DD, blank to skip)" "")
  end_date=$(prompt "End date (YYYY-MM-DD, blank to skip)" "")
  tx_type=$(prompt "Type filter (deposit/withdrawal/transfer, blank to skip)" "")
  status=$(prompt "Status filter (blank to skip)" "")

  local url="${GATEWAY_URL}/v1/transaction?bank_account_id=${id}&offset=${offset}&limit=${limit}"
  [[ -n "$start_date" ]] && url="${url}&start_date=${start_date}"
  [[ -n "$end_date" ]] && url="${url}&end_date=${end_date}"
  [[ -n "$tx_type" ]] && url="${url}&transaction_type=${tx_type}"
  [[ -n "$status" ]] && url="${url}&status=${status}"

  http_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"
}

action_balance_history() {
  echo -e "\n${CYAN}Balance History${NC}"
  separator
  api_key_check || return
  local id start_date end_date
  id=$(prompt "Bank account ID" "$LAST_ACCOUNT_ID")
  start_date=$(prompt "Start date (YYYY-MM-DD)" "2026-01-01")
  end_date=$(prompt "End date (YYYY-MM-DD)" "2026-12-31")

  http_get "${GATEWAY_URL}/v1/bank_account/${id}/balance-history?start_date=${start_date}&end_date=${end_date}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_ACCOUNT_ID="$id"

  echo -e "\n  ${YELLOW}Note: Snapshots are created by the midnight UTC cron job.${NC}"
}

action_settlement_report() {
  echo -e "\n${CYAN}Settlement Report (CSV)${NC}"
  separator
  api_key_check || return
  local id start_date end_date currency save
  id=$(prompt "Bank account ID (blank for all accounts)" "$LAST_ACCOUNT_ID")
  start_date=$(prompt "Start date (YYYY-MM-DD)" "2026-01-01")
  end_date=$(prompt "End date (YYYY-MM-DD)" "2026-12-31")
  currency=$(prompt "Currency (blank for auto)" "$LAST_CURRENCY")

  local url="${GATEWAY_URL}/v1/report/settlement?start_date=${start_date}&end_date=${end_date}"
  [[ -n "$id" ]] && url="${url}&bank_account_id=${id}"
  [[ -n "$currency" ]] && url="${url}&currency=${currency}"

  read -rp "  Save to file? (y/N): " save
  if [[ "$save" == "y" || "$save" == "Y" ]]; then
    local filename
    if [[ -n "$id" ]]; then
      filename="settlement_${id:0:8}_${start_date}_${end_date}.csv"
    else
      filename="settlement_all_${start_date}_${end_date}.csv"
    fi
    curl -s "$url" -H "$AUTH" -o "$filename"
    echo -e "  ${GREEN}Saved to ${filename}${NC}"
    echo ""
    head -5 "$filename"
    local lines
    lines=$(wc -l < "$filename")
    echo -e "  ${DIM}... (${lines} total lines)${NC}"
  else
    local response
    response=$(curl -s -w "\n%{http_code}" "$url" -H "$AUTH")
    HTTP_STATUS=$(echo "$response" | tail -1)
    HTTP_BODY=$(echo "$response" | sed '$d')
    print_status
    echo "$HTTP_BODY"
  fi

  LAST_ACCOUNT_ID="$id"
  LAST_CURRENCY="$currency"
}

action_quick_flow() {
  echo -e "\n${CYAN}Quick Flow: Open + Approve + Deposit${NC}"
  separator
  api_key_check || return
  echo -e "  ${DIM}This runs a complete account setup in one go.${NC}"
  echo ""
  local currency ext_ref amount
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  ext_ref=$(prompt "External reference ID (user ID)" "$LAST_USER_ID")
  amount=$(prompt "Initial deposit amount" "1000")
  LAST_CURRENCY="$currency"

  # 1. Open account
  echo -e "\n  ${BLUE}[1/3] Opening account...${NC}"
  local open_body
  open_body=$(cat <<ENDJSON
{
  "OpenAccount": {
    "account_type": "Retail",
    "kind": "Checking",
    "currency": "${currency}"$([ -n "$ext_ref" ] && echo ",
    \"external_reference_id\": \"${ext_ref}\"")
  }
}
ENDJSON
  )
  http_post "${GATEWAY_URL}/v1/bank_account" "$open_body"
  print_status

  local acct_id acct_num
  acct_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  acct_num=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('account_number',''))" 2>/dev/null)

  if [[ -z "$acct_id" ]]; then
    echo -e "  ${RED}Failed to open account.${NC}"
    pretty_json "$HTTP_BODY"
    return
  fi
  echo -e "  ${GREEN}Account: ${acct_id}${NC}"
  echo -e "  ${GREEN}Number:  ${acct_num}${NC}"
  LAST_ACCOUNT_ID="$acct_id"
  LAST_ACCOUNT_NUMBER="$acct_num"
  [[ -n "$ext_ref" ]] && LAST_USER_ID="$ext_ref"

  # 2. Approve
  echo -e "\n  ${BLUE}[2/3] Approving (KYC)...${NC}"
  http_post "${GATEWAY_URL}/v1/bank_account" "{\"ApproveAccount\":{\"id\":\"${acct_id}\"}}"
  print_status

  if [[ ! "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${RED}Approval failed.${NC}"
    pretty_json "$HTTP_BODY"
    return
  fi

  # Extract ledger_id
  local lid
  lid=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('ledger_id',''))" 2>/dev/null)
  [[ -n "$lid" ]] && LAST_LEDGER_ID="$lid"

  # 3. Deposit
  echo -e "\n  ${BLUE}[3/3] Depositing ${amount} ${currency}...${NC}"
  local dep_body
  dep_body=$(cat <<ENDJSON
{
  "Deposit": {
    "id": "${acct_id}",
    "amount": {
      "amount": "${amount}",
      "currency": "${currency}"
    }
  }
}
ENDJSON
  )
  http_post "${GATEWAY_URL}/v1/bank_account" "$dep_body"
  print_status

  echo -e "\n  ${GREEN}Done! Account is ready.${NC}"
  echo -e "  Account ID:  ${BOLD}${acct_id}${NC}"
  echo -e "  Ledger ID:   ${BOLD}${LAST_LEDGER_ID}${NC}"
  echo -e "  Account #:   ${BOLD}${acct_num}${NC}"
  echo -e "\n  ${YELLOW}Ledger balance updates in ~${OUTBOX_WAIT}s (outbox processing).${NC}"
}

action_list_accounts() {
  echo -e "\n${CYAN}List All Accounts${NC}"
  separator
  api_key_check || return

  http_get "${GATEWAY_URL}/v1/accounts?offset=0&limit=50"
  print_status

  if [[ ! "$HTTP_STATUS" =~ ^2 ]]; then
    pretty_json "$HTTP_BODY"
    return
  fi

  # Parse and display as a numbered table
  local count
  count=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
print(len(entries))
" 2>/dev/null)

  if [[ "$count" == "0" || -z "$count" ]]; then
    echo -e "  ${YELLOW}No accounts found. Use option 1 or 18 to create one.${NC}"
    return
  fi

  echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
pagination = data.get('pagination', {})
total = pagination.get('total', len(entries))

print(f'  {\"#\":>3}  {\"Account ID\":36}  {\"Account Number\":14}  {\"Status\":10}  {\"Currency\":8}  {\"Kind\":10}  {\"Available\":>15}')
print(f'  {\"---\":>3}  {\"-\"*36}  {\"-\"*14}  {\"-\"*10}  {\"-\"*8}  {\"-\"*10}  {\"-\"*15}')

for i, e in enumerate(entries):
    aid = e.get('id', '?')
    num = e.get('account_number', '?')
    st = e.get('status', '?')
    cur = e.get('currency', '?')
    kind = e.get('kind', '?')
    avail = e.get('available', '0')
    if avail is None: avail = '-'
    print(f'  {i+1:3}  {aid:36}  {num:14}  {st:10}  {cur:8}  {kind:10}  {str(avail):>15}')

print(f'\n  Total: {total} accounts')
"

  echo ""
  read -rp "  Select account # to set as active (Enter to skip): " pick
  if [[ -n "$pick" && "$pick" =~ ^[0-9]+$ ]]; then
    local selected
    selected=$(echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
entries = data.get('entries', [])
idx = int('${pick}') - 1
if 0 <= idx < len(entries):
    e = entries[idx]
    print(e.get('id', ''))
    print(e.get('ledger_id', ''))
    print(e.get('account_number', ''))
    print(e.get('currency', ''))
else:
    print('')
" 2>/dev/null)

    local sel_id sel_lid sel_num sel_cur
    sel_id=$(echo "$selected" | sed -n '1p')
    sel_lid=$(echo "$selected" | sed -n '2p')
    sel_num=$(echo "$selected" | sed -n '3p')
    sel_cur=$(echo "$selected" | sed -n '4p')

    if [[ -n "$sel_id" ]]; then
      LAST_ACCOUNT_ID="$sel_id"
      [[ -n "$sel_lid" ]] && LAST_LEDGER_ID="$sel_lid"
      [[ -n "$sel_num" ]] && LAST_ACCOUNT_NUMBER="$sel_num"
      [[ -n "$sel_cur" ]] && LAST_CURRENCY="$sel_cur"
      echo -e "  ${GREEN}Active account set:${NC}"
      echo -e "    Account ID: ${BOLD}${LAST_ACCOUNT_ID}${NC}"
      [[ -n "$LAST_LEDGER_ID" ]] && echo -e "    Ledger ID:  ${BOLD}${LAST_LEDGER_ID}${NC}"
      [[ -n "$LAST_ACCOUNT_NUMBER" ]] && echo -e "    Account #:  ${BOLD}${LAST_ACCOUNT_NUMBER}${NC}"
    else
      echo -e "  ${RED}Invalid selection.${NC}"
    fi
  fi
}

# ---------------------------------------------------------------------------
# Menu Actions — API Key Management (36-37)
# ---------------------------------------------------------------------------

action_switch_api_key() {
  echo -e "\n${CYAN}Switch API Key${NC}"
  separator

  if [[ -n "$API_KEY" ]]; then
    echo -e "  Current key: ${BOLD}${API_KEY:0:20}...${NC}"
  else
    echo -e "  Current key: ${RED}(none)${NC}"
  fi
  echo ""

  echo "  1) Enter API key manually"
  echo "  2) Create new key via portal (requires login)"
  read -rp "  Choice [1]: " choice
  choice="${choice:-1}"

  if [[ "$choice" == "2" ]]; then
    portal_check_session || return

    local name scopes
    name=$(prompt "Key name" "interactive-key")
    scopes="accounts:read,accounts:write,ledgers:read,transactions:read,house_accounts:read,house_accounts:write"

    local scopes_json
    scopes_json=$(echo "$scopes" | python3 -c "import sys; print('[' + ','.join(['\"'+s.strip()+'\"' for s in sys.stdin.read().strip().split(',')]) + ']')")

    portal_post "${GATEWAY_URL}/portal/v1/api-keys" \
      "{\"name\":\"${name}\",\"scopes\":${scopes_json}}"
    print_status

    if [[ "$HTTP_STATUS" =~ ^2 ]]; then
      local new_key
      new_key=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin)['raw_key'])" 2>/dev/null || echo "")
      if [[ -n "$new_key" ]]; then
        API_KEY="$new_key"
        AUTH="Authorization: Bearer ${API_KEY}"
        echo "$API_KEY" > .api-key
        echo -e "  ${GREEN}API key set: ${API_KEY:0:20}...${NC}"
        echo -e "  ${DIM}Saved to .api-key${NC}"
      else
        echo -e "  ${RED}Could not extract raw_key from response.${NC}"
        pretty_json "$HTTP_BODY"
      fi
    else
      echo -e "  ${RED}Failed to create key.${NC}"
      pretty_json "$HTTP_BODY"
    fi
  else
    local new_key
    read -rp "  API key (bk_live_...): " new_key
    if [[ -n "$new_key" ]]; then
      API_KEY="$new_key"
      AUTH="Authorization: Bearer ${API_KEY}"
      echo "$API_KEY" > .api-key
      echo -e "  ${GREEN}API key set: ${API_KEY:0:20}...${NC}"
    else
      echo -e "  ${RED}No key entered.${NC}"
    fi
  fi
}

action_show_api_key() {
  echo -e "\n${CYAN}Current API Key Info${NC}"
  separator

  if [[ -z "$API_KEY" ]]; then
    echo -e "  ${RED}No API key set.${NC}"
    echo -e "  ${YELLOW}Use option 36 to set one, or login (21) + create key (27).${NC}"
    return
  fi

  echo -e "  Key:    ${BOLD}${API_KEY:0:20}...${NC}"
  echo -e "  Prefix: ${API_KEY:0:8}"

  # If portal session exists, try to fetch key details
  if [[ -n "$PORTAL_LOGGED_IN" ]]; then
    portal_get "${GATEWAY_URL}/portal/v1/api-keys"
    if [[ "$HTTP_STATUS" =~ ^2 ]]; then
      echo ""
      echo "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
keys = data if isinstance(data, list) else data.get('entries', data.get('keys', []))
for k in keys:
    prefix = k.get('key_prefix', '')
    if prefix:
        print(f'  [{k.get(\"status\",\"?\")}] {k.get(\"name\",\"?\")} ({prefix}...) scopes: {\", \".join(k.get(\"scopes\",[]))}')
" 2>/dev/null
    fi
  fi
}

# ---------------------------------------------------------------------------
# Menu Actions — Portal API (21-35)
# ---------------------------------------------------------------------------

action_portal_login() {
  echo -e "\n${CYAN}Portal Login${NC}"
  separator
  local email password
  email=$(prompt "Email" "$PORTAL_EMAIL")
  read -rsp "  Password: " password
  echo ""

  local response
  response=$(curl -s -w "\n%{http_code}" -X POST \
    -c "$COOKIE_JAR" \
    -H "$CT" \
    -d "{\"email\":\"${email}\",\"password\":\"${password}\"}" \
    "${GATEWAY_URL}/portal/v1/auth/login")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    PORTAL_LOGGED_IN="yes"
    PORTAL_EMAIL="$email"
    echo -e "  ${GREEN}Logged in as ${email}${NC}"
    pretty_json "$HTTP_BODY"
  else
    echo -e "  ${RED}Login failed.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_signup() {
  echo -e "\n${CYAN}Portal Signup (Create Org + Owner)${NC}"
  separator
  local org_name name email password
  org_name=$(prompt "Organization name" "")
  name=$(prompt "Your name" "")
  email=$(prompt "Email" "")
  read -rsp "  Password: " password
  echo ""

  if [[ -z "$org_name" || -z "$email" || -z "$password" ]]; then
    echo -e "  ${RED}All fields are required.${NC}"
    return
  fi

  local response
  response=$(curl -s -w "\n%{http_code}" -X POST \
    -c "$COOKIE_JAR" \
    -H "$CT" \
    -d "{\"org_name\":\"${org_name}\",\"name\":\"${name}\",\"email\":\"${email}\",\"password\":\"${password}\"}" \
    "${GATEWAY_URL}/portal/v1/auth/signup")
  HTTP_STATUS=$(echo "$response" | tail -1)
  HTTP_BODY=$(echo "$response" | sed '$d')
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    PORTAL_LOGGED_IN="yes"
    PORTAL_EMAIL="$email"
    echo -e "  ${GREEN}Signed up and logged in as ${email}${NC}"
    pretty_json "$HTTP_BODY"
  else
    echo -e "  ${RED}Signup failed.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_dashboard() {
  echo -e "\n${CYAN}Portal Dashboard Stats${NC}"
  separator
  portal_check_session || return

  portal_get "${GATEWAY_URL}/portal/v1/dashboard/stats"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_activity() {
  echo -e "\n${CYAN}Portal Activity Feed${NC}"
  separator
  portal_check_session || return

  portal_get "${GATEWAY_URL}/portal/v1/dashboard/activity"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_org() {
  echo -e "\n${CYAN}Portal Organization Details${NC}"
  separator
  portal_check_session || return

  echo "  1) Get org details"
  echo "  2) List rate limits"
  read -rp "  Choice [1]: " choice
  choice="${choice:-1}"

  if [[ "$choice" == "2" ]]; then
    portal_get "${GATEWAY_URL}/portal/v1/dashboard/rate-limits"
  else
    # Get org_id from dashboard stats first
    portal_get "${GATEWAY_URL}/portal/v1/dashboard/stats"
    pretty_json "$HTTP_BODY"
  fi
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_list_keys() {
  echo -e "\n${CYAN}Portal: List API Keys${NC}"
  separator
  portal_check_session || return

  portal_get "${GATEWAY_URL}/portal/v1/api-keys"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_create_key() {
  echo -e "\n${CYAN}Portal: Create API Key${NC}"
  separator
  portal_check_session || return

  local name scopes
  name=$(prompt "Key name" "my-api-key")
  scopes=$(prompt "Scopes (comma-separated)" "accounts:read,accounts:write,ledgers:read,transactions:read,house_accounts:read,house_accounts:write")

  # Convert comma-separated to JSON array
  local scopes_json
  scopes_json=$(echo "$scopes" | python3 -c "import sys; print('[' + ','.join(['\"'+s.strip()+'\"' for s in sys.stdin.read().strip().split(',')]) + ']')")

  portal_post "${GATEWAY_URL}/portal/v1/api-keys" \
    "{\"name\":\"${name}\",\"scopes\":${scopes_json}}"
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}API key created. Save the raw_key — it won't be shown again!${NC}"
    pretty_json "$HTTP_BODY"

    # Offer to set as active key
    local new_key
    new_key=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('raw_key',''))" 2>/dev/null)
    if [[ -n "$new_key" ]]; then
      read -rp "  Set as active API key? (Y/n): " set_active
      set_active="${set_active:-Y}"
      if [[ "$set_active" == "Y" || "$set_active" == "y" ]]; then
        API_KEY="$new_key"
        AUTH="Authorization: Bearer ${API_KEY}"
        echo "$API_KEY" > .api-key
        echo -e "  ${GREEN}API key activated: ${API_KEY:0:20}...${NC}"
      fi
    fi
  else
    echo -e "  ${RED}Failed to create API key.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_rotate_key() {
  echo -e "\n${CYAN}Portal: Rotate API Key${NC}"
  separator
  portal_check_session || return

  local key_id
  key_id=$(prompt "API Key ID (UUID)" "")
  if [[ -z "$key_id" ]]; then
    echo -e "  ${RED}Key ID is required.${NC}"
    return
  fi

  portal_post "${GATEWAY_URL}/portal/v1/api-keys/${key_id}/rotate" "{}"
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}Key rotated. Save the new raw_key — it won't be shown again!${NC}"
    pretty_json "$HTTP_BODY"
  else
    echo -e "  ${RED}Failed to rotate key.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_revoke_key() {
  echo -e "\n${CYAN}Portal: Revoke API Key${NC}"
  separator
  portal_check_session || return

  local key_id
  key_id=$(prompt "API Key ID (UUID)" "")
  if [[ -z "$key_id" ]]; then
    echo -e "  ${RED}Key ID is required.${NC}"
    return
  fi

  portal_delete "${GATEWAY_URL}/portal/v1/api-keys/${key_id}"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_list_members() {
  echo -e "\n${CYAN}Portal: List Members${NC}"
  separator
  portal_check_session || return

  portal_get "${GATEWAY_URL}/portal/v1/members"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_invite_member() {
  echo -e "\n${CYAN}Portal: Invite Member${NC}"
  separator
  portal_check_session || return

  local email role name
  email=$(prompt "Invite email" "")
  role=$(prompt "Role (admin/member)" "member")
  name=$(prompt "Name (optional)" "")

  if [[ -z "$email" ]]; then
    echo -e "  ${RED}Email is required.${NC}"
    return
  fi

  local body="{\"email\":\"${email}\",\"role\":\"${role}\""
  [[ -n "$name" ]] && body="${body},\"name\":\"${name}\""
  body="${body}}"

  portal_post "${GATEWAY_URL}/portal/v1/members/invite" "$body"
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}Invite sent. Share the invite_link with the new member.${NC}"
    pretty_json "$HTTP_BODY"
  else
    echo -e "  ${RED}Failed to invite member.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_list_webhooks() {
  echo -e "\n${CYAN}Portal: List Webhook Endpoints${NC}"
  separator
  portal_check_session || return

  portal_get "${GATEWAY_URL}/portal/v1/webhooks"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_create_webhook() {
  echo -e "\n${CYAN}Portal: Create Webhook Endpoint${NC}"
  separator
  portal_check_session || return

  local url events desc
  url=$(prompt "Endpoint URL" "https://example.com/webhooks")
  events=$(prompt "Event types (comma-separated)" "account.opened,account.approved,transaction.completed")
  desc=$(prompt "Description (optional)" "")

  # Convert to JSON array
  local events_json
  events_json=$(echo "$events" | python3 -c "import sys; print('[' + ','.join(['\"'+s.strip()+'\"' for s in sys.stdin.read().strip().split(',')]) + ']')")

  local body="{\"url\":\"${url}\",\"event_types\":${events_json}"
  [[ -n "$desc" ]] && body="${body},\"description\":\"${desc}\""
  body="${body}}"

  portal_post "${GATEWAY_URL}/portal/v1/webhooks" "$body"
  print_status

  if [[ "$HTTP_STATUS" =~ ^2 ]]; then
    echo -e "  ${GREEN}Webhook created. Save the signing_secret — it won't be shown again!${NC}"
    pretty_json "$HTTP_BODY"
  else
    echo -e "  ${RED}Failed to create webhook.${NC}"
    pretty_json "$HTTP_BODY"
  fi
}

action_portal_api_logs() {
  echo -e "\n${CYAN}Portal: API Logs${NC}"
  separator
  portal_check_session || return

  local page per_page method status_code
  page=$(prompt "Page" "1")
  per_page=$(prompt "Per page" "20")
  method=$(prompt "Method filter (GET/POST/etc, blank to skip)" "")
  status_code=$(prompt "Status code filter (200/404/etc, blank to skip)" "")

  local url="${GATEWAY_URL}/portal/v1/logs?page=${page}&per_page=${per_page}"
  [[ -n "$method" ]] && url="${url}&method=${method}"
  [[ -n "$status_code" ]] && url="${url}&status_code=${status_code}"

  portal_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
}

action_portal_audit_logs() {
  echo -e "\n${CYAN}Portal: Audit Logs${NC}"
  separator
  portal_check_session || return

  local action_filter limit
  action_filter=$(prompt "Action filter (e.g. auth.login_success, blank for all)" "")
  limit=$(prompt "Limit" "20")

  local url="${GATEWAY_URL}/portal/v1/audit-logs?limit=${limit}&offset=0"
  [[ -n "$action_filter" ]] && url="${url}&action=${action_filter}"

  portal_get "$url"
  print_status
  pretty_json "$HTTP_BODY"
}

# ---------------------------------------------------------------------------
# Interest Engine Actions (38-44, via Gateway API key auth)
# ---------------------------------------------------------------------------

action_interest_list_rates() {
  echo -e "\n${CYAN}Interest: List Rate Configs${NC}"
  separator
  api_key_check || return

  local currency
  currency=$(prompt "Currency filter (blank for all)" "")

  local url="${GATEWAY_URL}/v1/interest/rates"
  [[ -n "$currency" ]] && url="${url}?currency=${currency}"

  http_get "$url"
  print_status
  pretty_json "$HTTP_BODY"

  # Remember first config ID if available
  local first_id
  first_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d['entries'][0]['id'] if d.get('entries') else '')" 2>/dev/null)
  [[ -n "$first_id" ]] && LAST_RATE_CONFIG_ID="$first_id"
}

action_interest_create_rate() {
  echo -e "\n${CYAN}Interest: Create Rate Config${NC}"
  separator
  api_key_check || return

  local currency day_count freq posting_day effective_from
  currency=$(prompt "Currency" "$LAST_CURRENCY")
  day_count=$(prompt "Day count convention (Actual/365, Actual/360, 30/360)" "Actual/365")
  freq=$(prompt "Posting frequency (Daily, Weekly, Monthly)" "Monthly")
  posting_day=$(prompt "Posting day (1-28 for Monthly, 1-7 for Weekly, blank for Daily)" "1")
  effective_from=$(prompt "Effective from (YYYY-MM-DD)" "$(date -u +%Y-%m-%d)")

  echo ""
  echo -e "  ${BOLD}Define tiers${NC} (blended rate — each tier covers a balance range)"
  echo -e "  ${DIM}Enter tiers one at a time. Last tier should have no max (unbounded).${NC}"

  local tiers_json="["
  local tier_order=1
  local prev_max="0"
  while true; do
    echo ""
    echo -e "  ${YELLOW}Tier ${tier_order}:${NC}"
    local min_bal max_bal apr
    min_bal=$(prompt "  Min balance" "$prev_max")
    max_bal=$(prompt "  Max balance (blank = unbounded/last tier)" "")
    apr=$(prompt "  APR (e.g. 0.045 for 4.5%)" "0.045")

    if [[ $tier_order -gt 1 ]]; then
      tiers_json="${tiers_json},"
    fi

    if [[ -z "$max_bal" ]]; then
      tiers_json="${tiers_json}{\"tier_order\":${tier_order},\"min_balance\":${min_bal},\"max_balance\":null,\"apr\":${apr}}"
      break
    else
      tiers_json="${tiers_json}{\"tier_order\":${tier_order},\"min_balance\":${min_bal},\"max_balance\":${max_bal},\"apr\":${apr}}"
      prev_max="$max_bal"
    fi

    tier_order=$((tier_order + 1))
  done
  tiers_json="${tiers_json}]"

  local posting_day_json="null"
  [[ -n "$posting_day" ]] && posting_day_json="$posting_day"

  local body
  body=$(cat <<EOF
{
  "currency": "${currency}",
  "day_count": "${day_count}",
  "posting_frequency": "${freq}",
  "posting_day": ${posting_day_json},
  "effective_from": "${effective_from}",
  "tiers": ${tiers_json}
}
EOF
)

  http_post "${GATEWAY_URL}/v1/interest/rates" "$body"
  print_status
  pretty_json "$HTTP_BODY"

  local config_id
  config_id=$(echo "$HTTP_BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null)
  [[ -n "$config_id" ]] && LAST_RATE_CONFIG_ID="$config_id"
}

action_interest_view_rate() {
  echo -e "\n${CYAN}Interest: View Rate Config${NC}"
  separator
  api_key_check || return

  local config_id
  config_id=$(prompt "Rate config ID" "$LAST_RATE_CONFIG_ID")
  [[ -z "$config_id" ]] && { echo -e "  ${RED}Config ID required.${NC}"; return; }

  http_get "${GATEWAY_URL}/v1/interest/rates/${config_id}"
  print_status
  pretty_json "$HTTP_BODY"
  LAST_RATE_CONFIG_ID="$config_id"
}

action_interest_sunset_rate() {
  echo -e "\n${CYAN}Interest: Sunset Rate Config${NC}"
  separator
  api_key_check || return

  local config_id effective_to deactivate
  config_id=$(prompt "Rate config ID" "$LAST_RATE_CONFIG_ID")
  [[ -z "$config_id" ]] && { echo -e "  ${RED}Config ID required.${NC}"; return; }

  effective_to=$(prompt "Effective to date (YYYY-MM-DD, blank to skip)" "")
  deactivate=$(prompt "Deactivate? (y/n)" "y")

  local is_active="true"
  [[ "$deactivate" == "y" || "$deactivate" == "Y" ]] && is_active="false"

  local body="{"
  [[ -n "$effective_to" ]] && body="${body}\"effective_to\":\"${effective_to}\","
  body="${body}\"is_active\":${is_active}}"

  http_put "${GATEWAY_URL}/v1/interest/rates/${config_id}" "$body"
  print_status
  pretty_json "$HTTP_BODY"
}

action_interest_replace_tiers() {
  echo -e "\n${CYAN}Interest: Replace Rate Tiers${NC}"
  separator
  api_key_check || return

  local config_id
  config_id=$(prompt "Rate config ID" "$LAST_RATE_CONFIG_ID")
  [[ -z "$config_id" ]] && { echo -e "  ${RED}Config ID required.${NC}"; return; }

  echo ""
  echo -e "  ${BOLD}Define new tiers${NC} (replaces all existing tiers atomically)"

  local tiers_json="["
  local tier_order=1
  local prev_max="0"
  while true; do
    echo ""
    echo -e "  ${YELLOW}Tier ${tier_order}:${NC}"
    local min_bal max_bal apr
    min_bal=$(prompt "  Min balance" "$prev_max")
    max_bal=$(prompt "  Max balance (blank = unbounded/last tier)" "")
    apr=$(prompt "  APR (e.g. 0.045 for 4.5%)" "0.045")

    if [[ $tier_order -gt 1 ]]; then
      tiers_json="${tiers_json},"
    fi

    if [[ -z "$max_bal" ]]; then
      tiers_json="${tiers_json}{\"tier_order\":${tier_order},\"min_balance\":${min_bal},\"max_balance\":null,\"apr\":${apr}}"
      break
    else
      tiers_json="${tiers_json}{\"tier_order\":${tier_order},\"min_balance\":${min_bal},\"max_balance\":${max_bal},\"apr\":${apr}}"
      prev_max="$max_bal"
    fi

    tier_order=$((tier_order + 1))
  done
  tiers_json="${tiers_json}]"

  http_put "${GATEWAY_URL}/v1/interest/rates/${config_id}/tiers" "{\"tiers\":${tiers_json}}"
  print_status
  pretty_json "$HTTP_BODY"
}

action_interest_accruals() {
  echo -e "\n${CYAN}Interest: View Accruals${NC}"
  separator
  api_key_check || return

  local account_id start_date end_date
  account_id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  [[ -z "$account_id" ]] && { echo -e "  ${RED}Account ID required.${NC}"; return; }

  start_date=$(prompt "Start date (YYYY-MM-DD)" "$(date -u -v-30d +%Y-%m-%d 2>/dev/null || date -u -d '30 days ago' +%Y-%m-%d 2>/dev/null)")
  end_date=$(prompt "End date (YYYY-MM-DD)" "$(date -u +%Y-%m-%d)")

  http_get "${GATEWAY_URL}/v1/interest/accruals?account_id=${account_id}&start_date=${start_date}&end_date=${end_date}"
  print_status
  pretty_json "$HTTP_BODY"
}

action_interest_postings() {
  echo -e "\n${CYAN}Interest: View Postings${NC}"
  separator
  api_key_check || return

  local account_id start_date end_date
  account_id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  [[ -z "$account_id" ]] && { echo -e "  ${RED}Account ID required.${NC}"; return; }

  start_date=$(prompt "Start date (YYYY-MM-DD)" "$(date -u -v-30d +%Y-%m-%d 2>/dev/null || date -u -d '30 days ago' +%Y-%m-%d 2>/dev/null)")
  end_date=$(prompt "End date (YYYY-MM-DD)" "$(date -u +%Y-%m-%d)")

  http_get "${GATEWAY_URL}/v1/interest/postings?account_id=${account_id}&start_date=${start_date}&end_date=${end_date}"
  print_status
  pretty_json "$HTTP_BODY"
}

action_interest_estimate() {
  echo -e "\n${CYAN}Interest: Estimate Interest${NC}"
  separator
  api_key_check || return

  local account_id days
  account_id=$(prompt "Account ID" "$LAST_ACCOUNT_ID")
  [[ -z "$account_id" ]] && { echo -e "  ${RED}Account ID required.${NC}"; return; }

  days=$(prompt "Days to estimate" "30")

  http_get "${GATEWAY_URL}/v1/interest/estimate?account_id=${account_id}&days=${days}"
  print_status
  pretty_json "$HTTP_BODY"
}

# ---------------------------------------------------------------------------
# Main Menu
# ---------------------------------------------------------------------------

print_menu() {
  echo ""
  echo -e "${BOLD}╔══════════════════════════════════════════════════════╗${NC}"
  echo -e "${BOLD}║            ${BLUE}Bankie Interactive Console${NC}${BOLD}                ║${NC}"
  echo -e "${BOLD}╚══════════════════════════════════════════════════════╝${NC}"
  if [[ -n "$API_KEY" ]]; then
    echo -e "  ${DIM}API Key: ${API_KEY:0:20}...${NC}"
  else
    echo -e "  ${DIM}API Key: ${RED}(none — use 21+27 or 36 to set)${NC}"
  fi
  echo ""
  echo -e "  ${BOLD}Account Lifecycle${NC}  ${DIM}(via Gateway, API key auth)${NC}"
  echo -e "    ${GREEN} 1${NC}) Open new account"
  echo -e "    ${GREEN} 2${NC}) Approve account (KYC)"
  echo -e "    ${GREEN} 3${NC}) View account details"
  echo -e "    ${GREEN} 4${NC}) Freeze / Unfreeze"
  echo -e "    ${GREEN} 5${NC}) Close account"
  echo ""
  echo -e "  ${BOLD}Money Operations${NC}"
  echo -e "    ${GREEN} 6${NC}) Deposit"
  echo -e "    ${GREEN} 7${NC}) Withdraw"
  echo -e "    ${GREEN} 8${NC}) Transfer"
  echo ""
  echo -e "  ${BOLD}Queries${NC}"
  echo -e "    ${GREEN} 9${NC}) View ledger balance"
  echo -e "    ${GREEN}10${NC}) List transactions"
  echo -e "    ${GREEN}11${NC}) List user accounts"
  echo -e "    ${GREEN}12${NC}) Lookup by account number"
  echo -e "    ${GREEN}13${NC}) List sub-accounts"
  echo ""
  echo -e "  ${BOLD}Reports${NC}"
  echo -e "    ${GREEN}14${NC}) Settlement report (CSV)"
  echo -e "    ${GREEN}15${NC}) Balance history"
  echo ""
  echo -e "  ${BOLD}House Accounts${NC}"
  echo -e "    ${GREEN}16${NC}) List house accounts"
  echo -e "    ${GREEN}17${NC}) Create house account"
  echo ""
  echo -e "  ${BOLD}Interest Engine${NC}"
  echo -e "    ${GREEN}38${NC}) List rate configs"
  echo -e "    ${GREEN}39${NC}) Create rate config"
  echo -e "    ${GREEN}40${NC}) View rate config"
  echo -e "    ${GREEN}41${NC}) Sunset rate config"
  echo -e "    ${GREEN}42${NC}) Replace rate tiers"
  echo -e "    ${GREEN}43${NC}) View accruals"
  echo -e "    ${GREEN}44${NC}) View postings"
  echo -e "    ${GREEN}45${NC}) Estimate interest"
  echo ""
  echo -e "  ${BOLD}Shortcuts${NC}"
  echo -e "    ${GREEN}18${NC}) Quick flow (open + approve + deposit)"
  echo -e "    ${GREEN}19${NC}) List all accounts"
  echo -e "    ${GREEN}20${NC}) Health check"
  echo ""
  echo -e "  ${BOLD}API Key${NC}"
  echo -e "    ${GREEN}36${NC}) Switch / set API key"
  echo -e "    ${GREEN}37${NC}) Show current API key info"
  echo ""
  echo -e "  ${BOLD}Portal Operations${NC}  ${DIM}(session auth)${NC}"
  echo -e "    ${GREEN}21${NC}) Portal login"
  echo -e "    ${GREEN}22${NC}) Portal signup (new org)"
  echo -e "    ${GREEN}23${NC}) Dashboard stats"
  echo -e "    ${GREEN}24${NC}) Activity feed"
  echo -e "    ${GREEN}25${NC}) Organization / rate limits"
  echo -e "    ${GREEN}26${NC}) List API keys"
  echo -e "    ${GREEN}27${NC}) Create API key"
  echo -e "    ${GREEN}28${NC}) Rotate API key"
  echo -e "    ${GREEN}29${NC}) Revoke API key"
  echo -e "    ${GREEN}30${NC}) List members"
  echo -e "    ${GREEN}31${NC}) Invite member"
  echo -e "    ${GREEN}32${NC}) List webhooks"
  echo -e "    ${GREEN}33${NC}) Create webhook"
  echo -e "    ${GREEN}34${NC}) API logs"
  echo -e "    ${GREEN}35${NC}) Audit logs"
  echo ""

  # Show remembered state
  if [[ -n "$LAST_ACCOUNT_ID" || -n "$LAST_LEDGER_ID" || -n "$LAST_USER_ID" || -n "$LAST_RATE_CONFIG_ID" || -n "$PORTAL_LOGGED_IN" ]]; then
    echo -e "  ${DIM}Remembered:${NC}"
    [[ -n "$LAST_ACCOUNT_ID" ]] && echo -e "    ${DIM}Account:  ${LAST_ACCOUNT_ID}${NC}"
    [[ -n "$LAST_LEDGER_ID" ]] && echo -e "    ${DIM}Ledger:   ${LAST_LEDGER_ID}${NC}"
    [[ -n "$LAST_USER_ID" ]] && echo -e "    ${DIM}User:     ${LAST_USER_ID}${NC}"
    [[ -n "$LAST_ACCOUNT_NUMBER" ]] && echo -e "    ${DIM}Acct #:   ${LAST_ACCOUNT_NUMBER}${NC}"
    [[ -n "$LAST_RATE_CONFIG_ID" ]] && echo -e "    ${DIM}Rate Cfg: ${LAST_RATE_CONFIG_ID}${NC}"
    [[ -n "$PORTAL_LOGGED_IN" ]] && echo -e "    ${DIM}Portal:   ${PORTAL_EMAIL} (logged in)${NC}"
    echo ""
  fi

  echo -e "    ${RED} 0${NC}) Exit"
  echo ""
}

# ---------------------------------------------------------------------------
# Main Loop
# ---------------------------------------------------------------------------
echo -e "\n${GREEN}Gateway at ${GATEWAY_URL}${NC}"
if [[ -n "$API_KEY" ]]; then
  echo -e "${GREEN}API Key: ${API_KEY:0:20}...${NC}"
else
  echo -e "${YELLOW}No API key set. Use option 21 (login) + 27 (create key) to get started.${NC}"
fi

while true; do
  print_menu
  read -rp "  Choose [0-45]: " choice

  case "$choice" in
    1)  action_open_account ;;
    2)  action_approve_account ;;
    3)  action_view_account ;;
    4)  action_freeze_unfreeze ;;
    5)  action_close_account ;;
    6)  action_deposit ;;
    7)  action_withdraw ;;
    8)  action_transfer ;;
    9)  action_view_ledger ;;
    10) action_transactions ;;
    11) action_list_user_accounts ;;
    12) action_lookup_by_number ;;
    13) action_sub_accounts ;;
    14) action_settlement_report ;;
    15) action_balance_history ;;
    16) action_list_house_accounts ;;
    17) action_create_house_account ;;
    18) action_quick_flow ;;
    19) action_list_accounts ;;
    20) action_health ;;
    21) action_portal_login ;;
    22) action_portal_signup ;;
    23) action_portal_dashboard ;;
    24) action_portal_activity ;;
    25) action_portal_org ;;
    26) action_portal_list_keys ;;
    27) action_portal_create_key ;;
    28) action_portal_rotate_key ;;
    29) action_portal_revoke_key ;;
    30) action_portal_list_members ;;
    31) action_portal_invite_member ;;
    32) action_portal_list_webhooks ;;
    33) action_portal_create_webhook ;;
    34) action_portal_api_logs ;;
    35) action_portal_audit_logs ;;
    36) action_switch_api_key ;;
    37) action_show_api_key ;;
    38) action_interest_list_rates ;;
    39) action_interest_create_rate ;;
    40) action_interest_view_rate ;;
    41) action_interest_sunset_rate ;;
    42) action_interest_replace_tiers ;;
    43) action_interest_accruals ;;
    44) action_interest_postings ;;
    45) action_interest_estimate ;;
    0|q|Q|exit)
      echo -e "\n${GREEN}Bye!${NC}\n"
      exit 0
      ;;
    *)
      echo -e "\n  ${RED}Invalid choice.${NC}"
      ;;
  esac

  echo ""
  read -rp "  Press Enter to continue..."
done
