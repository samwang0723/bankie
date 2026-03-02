import { useState } from "react";
import {
  BookOpen,
  Terminal,
  Shield,
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Copy,
  Check
} from "lucide-react";

type EndpointCategory =
  | "all"
  | "accounts"
  | "ledgers"
  | "transactions"
  | "house_accounts"
  | "reports";

interface Endpoint {
  method: "GET" | "POST" | "DELETE";
  path: string;
  scope: string;
  description: string;
  category: Exclude<EndpointCategory, "all">;
  queryParams?: {
    name: string;
    type: string;
    required: boolean;
    description: string;
  }[];
  bodyParams?: {
    name: string;
    type: string;
    required: boolean;
    description: string;
  }[];
  curlExample: string;
}

const ENDPOINTS: Endpoint[] = [
  {
    method: "GET",
    path: "/v1/accounts",
    scope: "accounts:read",
    description: "List all bank accounts (paginated)",
    category: "accounts",
    queryParams: [
      {
        name: "offset",
        type: "integer",
        required: false,
        description: "Pagination offset (default: 0)"
      },
      {
        name: "limit",
        type: "integer",
        required: false,
        description: "Page size, max 100 (default: 20)"
      }
    ],
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/accounts?offset=0&limit=20"`
  },
  {
    method: "POST",
    path: "/v1/bank_account",
    scope: "accounts:write",
    description:
      "Execute a bank account command (open, approve, deposit, withdraw, transfer, etc.)",
    category: "accounts",
    bodyParams: [
      {
        name: "OpenAccount",
        type: "object",
        required: false,
        description: "Open a new account"
      },
      {
        name: "  account_type",
        type: "string",
        required: true,
        description: '"Retail" | "Institution" | "Tax"'
      },
      {
        name: "  kind",
        type: "string",
        required: true,
        description: '"Checking" | "Interest" | "Yield"'
      },
      {
        name: "  currency",
        type: "string",
        required: true,
        description: '"USD" | "TWD" | "BTC" | "ETH" | "USDT"'
      },
      {
        name: "  external_reference_id",
        type: "string",
        required: false,
        description: "External user/reference ID"
      },
      {
        name: "ApproveAccount",
        type: "object",
        required: false,
        description: "Approve a pending account"
      },
      {
        name: "  id",
        type: "uuid",
        required: true,
        description: "Bank account ID"
      },
      {
        name: "Deposit",
        type: "object",
        required: false,
        description: "Deposit funds"
      },
      {
        name: "  id",
        type: "uuid",
        required: true,
        description: "Bank account ID"
      },
      {
        name: "  amount",
        type: "Money",
        required: true,
        description: '{"amount": "100.00", "currency": "USD"}'
      },
      {
        name: "Withdrawal",
        type: "object",
        required: false,
        description: "Withdraw funds (debit-hold pattern)"
      },
      {
        name: "  id",
        type: "uuid",
        required: true,
        description: "Bank account ID"
      },
      {
        name: "  amount",
        type: "Money",
        required: true,
        description: '{"amount": "50.00", "currency": "USD"}'
      },
      {
        name: "Transfer",
        type: "object",
        required: false,
        description: "Transfer between accounts"
      },
      {
        name: "  id",
        type: "uuid",
        required: true,
        description: "Source account ID"
      },
      {
        name: "  to_account_id",
        type: "uuid",
        required: true,
        description: "Destination account ID"
      },
      {
        name: "  amount",
        type: "Money",
        required: true,
        description: '{"amount": "25.00", "currency": "USD"}'
      }
    ],
    curlExample: `curl -X POST -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -H "Idempotency-Key: unique-key-123" \\
  -d '{
    "OpenAccount": {
      "account_type": "Retail",
      "kind": "Checking",
      "currency": "USD",
      "external_reference_id": "user-001"
    }
  }' \\
  "https://api.bankie.io/v1/bank_account"`
  },
  {
    method: "GET",
    path: "/v1/bank_account/:id",
    scope: "accounts:read",
    description: "Get account details by ID",
    category: "accounts",
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/bank_account/ACCOUNT_ID"`
  },
  {
    method: "GET",
    path: "/v1/bank_account/:id/sub-accounts",
    scope: "accounts:read",
    description: "List sub-accounts for a master account",
    category: "accounts",
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/bank_account/ACCOUNT_ID/sub-accounts"`
  },
  {
    method: "GET",
    path: "/v1/bank_account/by-number/:account_number",
    scope: "accounts:read",
    description: "Lookup account by account number",
    category: "accounts",
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/bank_account/by-number/1234567890"`
  },
  {
    method: "GET",
    path: "/v1/ledger/:id",
    scope: "ledgers:read",
    description: "Query ledger balances (available, pending, current)",
    category: "ledgers",
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/ledger/LEDGER_ID"`
  },
  {
    method: "GET",
    path: "/v1/transaction",
    scope: "transactions:read",
    description: "List transactions with optional filters",
    category: "transactions",
    queryParams: [
      {
        name: "bank_account_id",
        type: "uuid",
        required: false,
        description: "Filter by account ID"
      },
      {
        name: "offset",
        type: "integer",
        required: false,
        description: "Pagination offset (default: 0)"
      },
      {
        name: "limit",
        type: "integer",
        required: false,
        description: "Page size, max 100 (default: 20)"
      },
      {
        name: "start_date",
        type: "date",
        required: false,
        description: "Filter from date (YYYY-MM-DD)"
      },
      {
        name: "end_date",
        type: "date",
        required: false,
        description: "Filter to date (YYYY-MM-DD)"
      },
      {
        name: "transaction_type",
        type: "string",
        required: false,
        description: "Filter by type (deposit, withdrawal, transfer)"
      },
      {
        name: "status",
        type: "string",
        required: false,
        description: "Filter by status"
      }
    ],
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/transaction?bank_account_id=ACCOUNT_ID&limit=50"`
  },
  {
    method: "GET",
    path: "/v1/report/settlement",
    scope: "reports:read",
    description: "Settlement report (CSV download, max 90-day range)",
    category: "reports",
    queryParams: [
      {
        name: "start_date",
        type: "date",
        required: true,
        description: "Report start date (YYYY-MM-DD)"
      },
      {
        name: "end_date",
        type: "date",
        required: true,
        description: "Report end date (YYYY-MM-DD)"
      },
      {
        name: "bank_account_id",
        type: "uuid",
        required: false,
        description: "Specific account (default: all)"
      },
      {
        name: "currency",
        type: "string",
        required: false,
        description: "Currency filter"
      }
    ],
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  -o settlement.csv \\
  "https://api.bankie.io/v1/report/settlement?start_date=2026-01-01&end_date=2026-01-31"`
  },
  {
    method: "GET",
    path: "/v1/house_account",
    scope: "house_accounts:read",
    description: "List house accounts",
    category: "house_accounts",
    queryParams: [
      {
        name: "currency",
        type: "string",
        required: false,
        description: "Filter by currency (USD, TWD, BTC, etc.)"
      }
    ],
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/house_account?currency=USD"`
  },
  {
    method: "POST",
    path: "/v1/house_account",
    scope: "house_accounts:write",
    description: "Create a house (settlement) account",
    category: "house_accounts",
    bodyParams: [
      {
        name: "account_name",
        type: "string",
        required: true,
        description: "Display name for the house account"
      },
      {
        name: "account_type",
        type: "string",
        required: true,
        description: '"Settlement"'
      },
      {
        name: "currency",
        type: "string",
        required: true,
        description: '"USD" | "TWD" | "BTC" | "ETH" | "USDT"'
      },
      {
        name: "status",
        type: "string",
        required: true,
        description: '"active"'
      }
    ],
    curlExample: `curl -X POST -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  -H "Content-Type: application/json" \\
  -H "Idempotency-Key: unique-key-456" \\
  -d '{
    "account_name": "Master USD Account",
    "account_type": "Settlement",
    "currency": "USD",
    "status": "active"
  }' \\
  "https://api.bankie.io/v1/house_account"`
  },
  {
    method: "GET",
    path: "/v1/bank_account/:id/balance-history",
    scope: "accounts:read",
    description: "Balance history from daily snapshots",
    category: "accounts",
    queryParams: [
      {
        name: "start_date",
        type: "date",
        required: true,
        description: "History start date (YYYY-MM-DD)"
      },
      {
        name: "end_date",
        type: "date",
        required: true,
        description: "History end date (YYYY-MM-DD)"
      }
    ],
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/bank_account/ACCOUNT_ID/balance-history?start_date=2026-01-01&end_date=2026-01-31"`
  },
  {
    method: "GET",
    path: "/v1/user/:id",
    scope: "accounts:read",
    description: "Query user accounts with ledger balances",
    category: "accounts",
    curlExample: `curl -H "Authorization: Bearer bk_live_YOUR_API_KEY" \\
  "https://api.bankie.io/v1/user/USER_ID"`
  }
];

const FILTER_PILLS: { label: string; value: EndpointCategory }[] = [
  { label: "All", value: "all" },
  { label: "Accounts", value: "accounts" },
  { label: "Ledgers", value: "ledgers" },
  { label: "Transactions", value: "transactions" },
  { label: "House Accounts", value: "house_accounts" },
  { label: "Reports", value: "reports" }
];

const ERROR_CODES = [
  {
    code: "400",
    name: "Bad Request",
    description: "The request body or parameters are invalid."
  },
  {
    code: "401",
    name: "Unauthorized",
    description: "Missing or invalid API key."
  },
  {
    code: "403",
    name: "Forbidden",
    description: "API key lacks the required scope for this endpoint."
  },
  {
    code: "404",
    name: "Not Found",
    description: "The requested resource does not exist."
  },
  {
    code: "409",
    name: "Conflict",
    description: "Duplicate idempotency key or conflicting state."
  },
  {
    code: "422",
    name: "Unprocessable Entity",
    description: "Request is well-formed but semantically invalid."
  },
  {
    code: "429",
    name: "Too Many Requests",
    description: "Rate limit exceeded. Check Retry-After header."
  }
];

function HighlightedCurl({ text }: { text: string }) {
  // Tokenize curl command for syntax highlighting matching Quick Start style
  const tokens: { value: string; className: string }[] = [];
  let remaining = text;

  while (remaining.length > 0) {
    // Match curl command word
    const curlMatch = remaining.match(/^(curl)\b/);
    if (curlMatch) {
      tokens.push({ value: curlMatch[1], className: "text-cyan-400" });
      remaining = remaining.slice(curlMatch[1].length);
      continue;
    }

    // Match flags like -H, -X, -d, -o
    const flagMatch = remaining.match(/^(-[A-Za-z]+)/);
    if (flagMatch) {
      tokens.push({ value: flagMatch[1], className: "text-cyan-400" });
      remaining = remaining.slice(flagMatch[1].length);
      continue;
    }

    // Match double-quoted strings
    const dqMatch = remaining.match(/^("(?:[^"\\]|\\.)*")/);
    if (dqMatch) {
      tokens.push({ value: dqMatch[1], className: "text-green-400" });
      remaining = remaining.slice(dqMatch[1].length);
      continue;
    }

    // Match single-quoted strings (JSON bodies)
    const sqMatch = remaining.match(/^('(?:[^'\\]|\\.)*')/s);
    if (sqMatch) {
      tokens.push({ value: sqMatch[1], className: "text-green-400" });
      remaining = remaining.slice(sqMatch[1].length);
      continue;
    }

    // Match URLs (http:// or https://)
    const urlMatch = remaining.match(/^(https?:\/\/[^\s"']+)/);
    if (urlMatch) {
      tokens.push({ value: urlMatch[1], className: "text-slate-400" });
      remaining = remaining.slice(urlMatch[1].length);
      continue;
    }

    // Default: plain text (whitespace, backslashes, newlines)
    const plainMatch = remaining.match(/^([^a-zA-Z"'h-]+|[a-zA-Z]+)/);
    if (plainMatch) {
      tokens.push({ value: plainMatch[0], className: "text-slate-300" });
      remaining = remaining.slice(plainMatch[0].length);
    } else {
      tokens.push({ value: remaining[0], className: "text-slate-300" });
      remaining = remaining.slice(1);
    }
  }

  return (
    <pre className="text-sm font-mono text-slate-300 leading-relaxed whitespace-pre">
      {tokens.map((token, i) => (
        <span key={i} className={token.className}>
          {token.value}
        </span>
      ))}
    </pre>
  );
}

function MethodBadge({ method }: { method: string }) {
  const styles: Record<string, string> = {
    GET: "bg-green-50 text-green-700",
    POST: "bg-cyan-50 text-cyan-700",
    DELETE: "bg-red-50 text-red-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded text-xs font-bold font-mono ${
        styles[method] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {method}
    </span>
  );
}

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  async function handleCopy() {
    await navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <button
      type="button"
      onClick={handleCopy}
      className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-md border border-slate-200
        hover:bg-slate-50 transition-colors"
      title="Copy curl command"
    >
      {copied ? (
        <>
          <Check className="w-3.5 h-3.5 text-green-600" />
          <span className="text-green-600">Copied</span>
        </>
      ) : (
        <>
          <Copy className="w-3.5 h-3.5 text-slate-500" />
          <span className="text-slate-600">Copy</span>
        </>
      )}
    </button>
  );
}

function EndpointRow({ endpoint }: { endpoint: Endpoint }) {
  const [expanded, setExpanded] = useState(false);
  const hasDetails =
    (endpoint.queryParams && endpoint.queryParams.length > 0) ||
    (endpoint.bodyParams && endpoint.bodyParams.length > 0) ||
    endpoint.curlExample;

  return (
    <>
      <tr
        className={`hover:bg-slate-50 cursor-pointer ${expanded ? "bg-slate-50" : ""}`}
        onClick={() => hasDetails && setExpanded(!expanded)}
      >
        <td className="px-6 py-3 w-[32px]">
          {hasDetails &&
            (expanded ? (
              <ChevronDown className="w-4 h-4 text-slate-400" />
            ) : (
              <ChevronRight className="w-4 h-4 text-slate-400" />
            ))}
        </td>
        <td className="px-6 py-3">
          <MethodBadge method={endpoint.method} />
        </td>
        <td className="px-6 py-3 text-sm font-mono text-slate-900">
          {endpoint.path}
        </td>
        <td className="px-6 py-3">
          <span className="inline-flex items-center px-2 py-0.5 rounded bg-slate-100 text-xs font-mono text-slate-600">
            {endpoint.scope}
          </span>
        </td>
        <td className="px-6 py-3 text-sm text-slate-600">
          {endpoint.description}
        </td>
      </tr>
      {expanded && (
        <tr>
          <td colSpan={5} className="px-6 py-4 bg-slate-50">
            <div className="ml-8 space-y-4">
              {/* Query Parameters */}
              {endpoint.queryParams && endpoint.queryParams.length > 0 && (
                <div>
                  <h4 className="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-2">
                    Query Parameters
                  </h4>
                  <div className="bg-white rounded-lg border border-slate-200">
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="border-b border-slate-100">
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Name
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Type
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Required
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Description
                          </th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-slate-100">
                        {endpoint.queryParams.map((param) => (
                          <tr key={param.name}>
                            <td className="px-4 py-2 font-mono text-xs text-slate-800">
                              {param.name}
                            </td>
                            <td className="px-4 py-2 text-xs text-slate-500">
                              {param.type}
                            </td>
                            <td className="px-4 py-2 text-xs">
                              {param.required ? (
                                <span className="text-red-500 font-medium">
                                  required
                                </span>
                              ) : (
                                <span className="text-slate-400">optional</span>
                              )}
                            </td>
                            <td className="px-4 py-2 text-xs text-slate-600">
                              {param.description}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}

              {/* Body Parameters */}
              {endpoint.bodyParams && endpoint.bodyParams.length > 0 && (
                <div>
                  <h4 className="text-xs font-semibold text-slate-500 uppercase tracking-wider mb-2">
                    Request Body (JSON)
                  </h4>
                  <div className="bg-white rounded-lg border border-slate-200">
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="border-b border-slate-100">
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Field
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Type
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Required
                          </th>
                          <th className="text-left px-4 py-2 text-[11px] font-semibold text-slate-400 uppercase font-mono">
                            Description
                          </th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-slate-100">
                        {endpoint.bodyParams.map((param, i) => {
                          const isHeader =
                            param.name === param.name.trimStart();
                          return (
                            <tr
                              key={i}
                              className={isHeader ? "bg-slate-50/50" : ""}
                            >
                              <td className="px-4 py-2 font-mono text-xs text-slate-800">
                                {isHeader ? (
                                  <span className="font-semibold">
                                    {param.name}
                                  </span>
                                ) : (
                                  <span className="pl-2 text-slate-600">
                                    {param.name.trim()}
                                  </span>
                                )}
                              </td>
                              <td className="px-4 py-2 text-xs text-slate-500">
                                {param.type}
                              </td>
                              <td className="px-4 py-2 text-xs">
                                {param.required ? (
                                  <span className="text-red-500 font-medium">
                                    required
                                  </span>
                                ) : (
                                  <span className="text-slate-400">
                                    optional
                                  </span>
                                )}
                              </td>
                              <td className="px-4 py-2 text-xs text-slate-600">
                                {param.description}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </div>
              )}

              {/* curl Example */}
              {endpoint.curlExample && (
                <div>
                  <div className="flex items-center justify-between mb-2">
                    <h4 className="text-xs font-semibold text-slate-500 uppercase tracking-wider">
                      Example Request
                    </h4>
                    <CopyButton text={endpoint.curlExample} />
                  </div>
                  <div className="bg-[#0A0F1C] rounded-lg p-4 overflow-x-auto">
                    <HighlightedCurl text={endpoint.curlExample} />
                  </div>
                </div>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

export function ApiDocs() {
  const [filter, setFilter] = useState<EndpointCategory>("all");

  const filteredEndpoints =
    filter === "all"
      ? ENDPOINTS
      : ENDPOINTS.filter((e) => e.category === filter);

  return (
    <div>
      {/* Page header */}
      <div className="mb-8">
        <div className="flex items-center gap-3 mb-1">
          <BookOpen className="w-6 h-6 text-cyan-400" />
          <h1 className="text-2xl font-bold text-slate-900">
            API Documentation
          </h1>
        </div>
        <p className="mt-1 text-sm text-slate-500">
          Everything you need to integrate with the Bankie Banking API.
        </p>
      </div>

      {/* Quick Start */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 mb-6">
        <div className="flex items-center gap-2 mb-4">
          <Terminal className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">
            Quick Start
          </h2>
        </div>
        <p className="text-sm text-slate-500 mb-4">
          Authenticate your requests by including your API key in the
          Authorization header.
        </p>
        <div className="bg-[#0A0F1C] rounded-lg p-4 overflow-x-auto">
          <pre className="text-sm font-mono text-slate-300 leading-relaxed">
            <span className="text-cyan-400">curl</span>
            {" -H "}
            <span className="text-green-400">
              &quot;Authorization: Bearer bk_live_your_api_key&quot;
            </span>
            {" \\\n  "}
            <span className="text-slate-400">
              https://api.bankie.io/v1/accounts
            </span>
          </pre>
        </div>
      </div>

      {/* API Endpoints */}
      <div className="mb-6">
        <h2 className="text-base font-semibold text-slate-900 mb-4">
          API Endpoints
        </h2>

        {/* Filter pills */}
        <div className="flex flex-wrap gap-2 mb-4">
          {FILTER_PILLS.map((pill) => (
            <button
              key={pill.value}
              onClick={() => setFilter(pill.value)}
              className={`px-3 py-1.5 text-xs font-medium rounded-full transition-colors ${
                filter === pill.value
                  ? "bg-cyan-400 text-[#0A0F1C]"
                  : "bg-slate-100 text-slate-600 hover:bg-slate-200"
              }`}
            >
              {pill.label}
            </button>
          ))}
        </div>

        <p className="text-xs text-slate-400 mb-3">
          Click any endpoint to view parameters and sample curl command.
        </p>

        {/* Endpoints table */}
        <div className="bg-white rounded-xl border border-slate-200">
          <table className="w-full" aria-label="API endpoints">
            <thead>
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
                <th className="w-[32px] px-6 py-3" />
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[80px]">
                  Method
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Path
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Scope
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Description
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {filteredEndpoints.map((endpoint, i) => (
                <EndpointRow key={i} endpoint={endpoint} />
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Authentication */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 mb-6">
        <div className="flex items-center gap-2 mb-4">
          <Shield className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">
            Authentication
          </h2>
        </div>
        <div className="space-y-3 text-sm text-slate-600">
          <p>
            All API requests require a valid API key passed in the{" "}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              Authorization
            </code>{" "}
            header using the Bearer scheme.
          </p>
          <div className="bg-slate-50 rounded-lg p-3 font-mono text-xs text-slate-700">
            Authorization: Bearer bk_live_xxxxxxxxxxxx
          </div>
          <p>
            API keys are scoped to specific permissions. Ensure your key has the
            required scope for each endpoint. Requests with insufficient scopes
            will receive a{" "}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              403 Forbidden
            </code>{" "}
            response.
          </p>
          <p>
            For mutating requests (POST, PUT, DELETE), include an{" "}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              Idempotency-Key
            </code>{" "}
            header with a unique value to prevent duplicate operations. Keys are
            valid for 24 hours.
          </p>
        </div>
      </div>

      {/* Error Codes */}
      <div className="bg-white rounded-xl border border-slate-200 p-6">
        <div className="flex items-center gap-2 mb-4">
          <AlertTriangle className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">
            Error Codes
          </h2>
        </div>
        <p className="text-sm text-slate-500 mb-4">
          All errors return a JSON body with{" "}
          <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
            code
          </code>{" "}
          and{" "}
          <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
            message
          </code>{" "}
          fields.
        </p>
        <div className="space-y-3">
          {ERROR_CODES.map((error) => (
            <div key={error.code} className="flex items-start gap-4">
              <span className="inline-flex items-center px-2.5 py-0.5 rounded bg-red-50 text-red-700 text-xs font-bold font-mono shrink-0">
                {error.code}
              </span>
              <div>
                <p className="text-sm font-medium text-slate-900">
                  {error.name}
                </p>
                <p className="text-sm text-slate-500">{error.description}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
