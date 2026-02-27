import { useState } from 'react';
import { BookOpen, Terminal, Shield, AlertTriangle } from 'lucide-react';

type EndpointCategory = 'all' | 'accounts' | 'ledgers' | 'transactions' | 'house_accounts' | 'reports';

interface Endpoint {
  method: 'GET' | 'POST' | 'DELETE';
  path: string;
  scope: string;
  description: string;
  category: Exclude<EndpointCategory, 'all'>;
}

const ENDPOINTS: Endpoint[] = [
  { method: 'GET', path: '/v1/accounts', scope: 'accounts:read', description: 'List all bank accounts', category: 'accounts' },
  { method: 'POST', path: '/v1/bank_account', scope: 'accounts:write', description: 'Execute account command', category: 'accounts' },
  { method: 'GET', path: '/v1/bank_account/:id', scope: 'accounts:read', description: 'Get account details', category: 'accounts' },
  { method: 'GET', path: '/v1/ledger/:id', scope: 'ledgers:read', description: 'Query ledger balances', category: 'ledgers' },
  { method: 'GET', path: '/v1/transaction', scope: 'transactions:read', description: 'List transactions', category: 'transactions' },
  { method: 'GET', path: '/v1/report/settlement', scope: 'reports:read', description: 'Settlement report (CSV)', category: 'reports' },
  { method: 'GET', path: '/v1/house_account', scope: 'house_accounts:read', description: 'List house accounts', category: 'house_accounts' },
  { method: 'POST', path: '/v1/house_account', scope: 'house_accounts:write', description: 'Create house account', category: 'house_accounts' },
];

const FILTER_PILLS: { label: string; value: EndpointCategory }[] = [
  { label: 'All', value: 'all' },
  { label: 'Accounts', value: 'accounts' },
  { label: 'Ledgers', value: 'ledgers' },
  { label: 'Transactions', value: 'transactions' },
  { label: 'House Accounts', value: 'house_accounts' },
  { label: 'Reports', value: 'reports' },
];

const ERROR_CODES = [
  { code: '400', name: 'Bad Request', description: 'The request body or parameters are invalid.' },
  { code: '401', name: 'Unauthorized', description: 'Missing or invalid API key.' },
  { code: '403', name: 'Forbidden', description: 'API key lacks the required scope for this endpoint.' },
  { code: '404', name: 'Not Found', description: 'The requested resource does not exist.' },
  { code: '409', name: 'Conflict', description: 'Duplicate idempotency key or conflicting state.' },
  { code: '422', name: 'Unprocessable Entity', description: 'Request is well-formed but semantically invalid.' },
];

function MethodBadge({ method }: { method: string }) {
  const styles: Record<string, string> = {
    GET: 'bg-green-50 text-green-700',
    POST: 'bg-cyan-50 text-cyan-700',
    DELETE: 'bg-red-50 text-red-700',
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded text-xs font-bold font-mono ${
        styles[method] ?? 'bg-slate-100 text-slate-600'
      }`}
    >
      {method}
    </span>
  );
}

export function ApiDocs() {
  const [filter, setFilter] = useState<EndpointCategory>('all');

  const filteredEndpoints =
    filter === 'all' ? ENDPOINTS : ENDPOINTS.filter((e) => e.category === filter);

  return (
    <div>
      {/* Page header */}
      <div className="mb-8">
        <div className="flex items-center gap-3 mb-1">
          <BookOpen className="w-6 h-6 text-cyan-400" />
          <h1 className="text-2xl font-bold text-slate-900">API Documentation</h1>
        </div>
        <p className="mt-1 text-sm text-slate-500">
          Everything you need to integrate with the Bankie Banking API.
        </p>
      </div>

      {/* Quick Start */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 mb-6">
        <div className="flex items-center gap-2 mb-4">
          <Terminal className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">Quick Start</h2>
        </div>
        <p className="text-sm text-slate-500 mb-4">
          Authenticate your requests by including your API key in the Authorization header.
        </p>
        <div className="bg-[#0A0F1C] rounded-lg p-4 overflow-x-auto">
          <pre className="text-sm font-mono text-slate-300 leading-relaxed">
            <span className="text-cyan-400">curl</span>{' -H '}
            <span className="text-green-400">"Authorization: Bearer bnk_live_your_api_key"</span>
            {' \\\n  '}
            <span className="text-slate-400">https://api.bankie.io/v1/accounts</span>
          </pre>
        </div>
      </div>

      {/* API Endpoints */}
      <div className="mb-6">
        <h2 className="text-base font-semibold text-slate-900 mb-4">API Endpoints</h2>

        {/* Filter pills */}
        <div className="flex flex-wrap gap-2 mb-4">
          {FILTER_PILLS.map((pill) => (
            <button
              key={pill.value}
              onClick={() => setFilter(pill.value)}
              className={`px-3 py-1.5 text-xs font-medium rounded-full transition-colors ${
                filter === pill.value
                  ? 'bg-cyan-400 text-[#0A0F1C]'
                  : 'bg-slate-100 text-slate-600 hover:bg-slate-200'
              }`}
            >
              {pill.label}
            </button>
          ))}
        </div>

        {/* Endpoints table */}
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <table className="w-full" aria-label="API endpoints">
            <thead>
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
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
                <tr key={i} className="hover:bg-slate-50">
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
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* Authentication */}
      <div className="bg-white rounded-xl border border-slate-200 p-6 mb-6">
        <div className="flex items-center gap-2 mb-4">
          <Shield className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">Authentication</h2>
        </div>
        <div className="space-y-3 text-sm text-slate-600">
          <p>
            All API requests require a valid API key passed in the{' '}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              Authorization
            </code>{' '}
            header using the Bearer scheme.
          </p>
          <div className="bg-slate-50 rounded-lg p-3 font-mono text-xs text-slate-700">
            Authorization: Bearer bnk_live_xxxxxxxxxxxx
          </div>
          <p>
            API keys are scoped to specific permissions. Ensure your key has the required scope
            for each endpoint. Requests with insufficient scopes will receive a{' '}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              403 Forbidden
            </code>{' '}
            response.
          </p>
          <p>
            For mutating requests (POST, PUT, DELETE), include an{' '}
            <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
              Idempotency-Key
            </code>{' '}
            header with a unique value to prevent duplicate operations. Keys are valid for 24 hours.
          </p>
        </div>
      </div>

      {/* Error Codes */}
      <div className="bg-white rounded-xl border border-slate-200 p-6">
        <div className="flex items-center gap-2 mb-4">
          <AlertTriangle className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold text-slate-900">Error Codes</h2>
        </div>
        <p className="text-sm text-slate-500 mb-4">
          All errors return a JSON body with{' '}
          <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
            code
          </code>{' '}
          and{' '}
          <code className="px-1.5 py-0.5 bg-slate-100 rounded text-xs font-mono text-slate-800">
            message
          </code>{' '}
          fields.
        </p>
        <div className="space-y-3">
          {ERROR_CODES.map((error) => (
            <div key={error.code} className="flex items-start gap-4">
              <span className="inline-flex items-center px-2.5 py-0.5 rounded bg-red-50 text-red-700 text-xs font-bold font-mono shrink-0">
                {error.code}
              </span>
              <div>
                <p className="text-sm font-medium text-slate-900">{error.name}</p>
                <p className="text-sm text-slate-500">{error.description}</p>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
