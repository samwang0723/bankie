import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Wallet, Search, Eye, ChevronDown, ChevronRight } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import type { BankAccountView } from "../types/index.ts";

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    Approved: "bg-green-50 text-green-700",
    Pending: "bg-amber-50 text-amber-700",
    Freeze: "bg-red-50 text-red-700",
    CustomerClosed: "bg-slate-100 text-slate-600"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
        styles[status] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {status}
    </span>
  );
}

function KindBadge({ kind }: { kind: string }) {
  const styles: Record<string, string> = {
    Checking: "bg-cyan-50 text-cyan-700",
    Interest: "bg-purple-50 text-purple-700",
    Yield: "bg-indigo-50 text-indigo-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
        styles[kind] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {kind}
    </span>
  );
}

function formatDate(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric"
  });
}

export function Accounts() {
  const [search, setSearch] = useState("");
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const { data: accounts = [], isLoading } = useQuery({
    queryKey: ["accounts"],
    queryFn: async () => {
      try {
        const resp = await api.get<{
          entries: BankAccountView[];
          pagination: { total: number };
        }>("/data/accounts?offset=0&limit=100");
        return resp.entries;
      } catch (err) {
        handleApiError(err);
        return [];
      }
    }
  });

  const filtered = accounts.filter((a) => {
    if (!search) return true;
    const term = search.toLowerCase();
    return (
      a.account_number.toLowerCase().includes(term) ||
      a.kind.toLowerCase().includes(term) ||
      a.currency.toLowerCase().includes(term) ||
      a.status.toLowerCase().includes(term)
    );
  });

  const totalCount = accounts.length;
  const activeCount = accounts.filter((a) => a.status === "Approved").length;
  const pendingCount = accounts.filter((a) => a.status === "Pending").length;
  const frozenCount = accounts.filter((a) => a.status === "Freeze").length;

  return (
    <div>
      {/* Page header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <div className="flex items-center gap-3 mb-1">
            <Wallet className="w-6 h-6 text-cyan-400" />
            <h1 className="text-2xl font-bold text-slate-900">Accounts</h1>
          </div>
          <p className="mt-1 text-sm text-slate-500">
            View and manage bank accounts for your organization.
          </p>
        </div>
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
          <input
            type="text"
            placeholder="Search accounts..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-9 pr-4 h-10 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
              placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent w-64"
          />
        </div>
      </div>

      {/* Stats row */}
      {isLoading ? (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
          {[1, 2, 3, 4].map((i) => (
            <div
              key={i}
              className="bg-white rounded-xl border border-slate-200 p-5 animate-pulse"
            >
              <div className="h-4 bg-slate-200 rounded w-20 mb-3" />
              <div className="h-8 bg-slate-200 rounded w-12" />
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-sm font-medium text-slate-500">Total Accounts</p>
            <p className="mt-2 text-3xl font-semibold text-slate-900">
              {totalCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-sm font-medium text-slate-500">Active</p>
            <p className="mt-2 text-3xl font-semibold text-green-600">
              {activeCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-sm font-medium text-slate-500">Pending</p>
            <p className="mt-2 text-3xl font-semibold text-amber-600">
              {pendingCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-sm font-medium text-slate-500">Frozen</p>
            <p className="mt-2 text-3xl font-semibold text-red-600">
              {frozenCount}
            </p>
          </div>
        </div>
      )}

      {/* Accounts table */}
      {isLoading ? (
        <div className="bg-white rounded-xl border border-slate-200">
          {[1, 2, 3].map((i) => (
            <div
              key={i}
              className="p-4 border-b border-slate-100 animate-pulse"
            >
              <div className="flex items-center gap-4">
                <div className="h-4 bg-slate-200 rounded w-32" />
                <div className="h-4 bg-slate-200 rounded w-24" />
                <div className="h-4 bg-slate-200 rounded w-16" />
                <div className="h-4 bg-slate-200 rounded w-20" />
              </div>
            </div>
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <div className="bg-white rounded-xl border border-slate-200 p-12 text-center">
          <Wallet className="w-10 h-10 text-slate-300 mx-auto mb-3" />
          <p className="text-slate-500 mb-1">
            {search ? "No accounts match your search" : "No accounts found"}
          </p>
          <p className="text-sm text-slate-400">
            {search
              ? "Try adjusting your search term."
              : "Accounts will appear here once created via the API."}
          </p>
        </div>
      ) : (
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <table className="w-full" aria-label="Bank accounts">
            <thead>
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-10" />
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Account Number
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Type
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Currency
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Status
                </th>
                <th className="text-right px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[80px]">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {filtered.map((account) => (
                <AccountRow
                  key={account.id}
                  account={account}
                  isExpanded={expandedId === account.id}
                  onToggleExpand={() =>
                    setExpandedId(expandedId === account.id ? null : account.id)
                  }
                />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function AccountRow({
  account,
  isExpanded,
  onToggleExpand
}: {
  account: BankAccountView;
  isExpanded: boolean;
  onToggleExpand: () => void;
}) {
  return (
    <>
      <tr
        className="hover:bg-slate-50 cursor-pointer h-14"
        onClick={onToggleExpand}
        aria-expanded={isExpanded}
      >
        <td className="px-6 py-3 text-slate-400">
          {isExpanded ? (
            <ChevronDown className="w-4 h-4" />
          ) : (
            <ChevronRight className="w-4 h-4" />
          )}
        </td>
        <td className="px-6 py-3 text-sm font-mono font-medium text-slate-900">
          {account.account_number}
        </td>
        <td className="px-6 py-3">
          <KindBadge kind={account.kind} />
        </td>
        <td className="px-6 py-3 text-sm font-medium text-slate-900">
          {account.currency}
        </td>
        <td className="px-6 py-3">
          <StatusBadge status={account.status} />
        </td>
        <td className="px-6 py-3 text-right">
          <button
            onClick={(e) => {
              e.stopPropagation();
              onToggleExpand();
            }}
            className="p-1.5 text-slate-400 hover:text-cyan-500 transition-colors"
            aria-label={`View details for ${account.account_number}`}
          >
            <Eye className="w-4 h-4" />
          </button>
        </td>
      </tr>
      {isExpanded && (
        <tr>
          <td
            colSpan={6}
            className="px-6 py-4 bg-slate-50 border-t border-slate-100"
          >
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
              <div>
                <p className="text-slate-500">Account ID</p>
                <p className="font-mono text-slate-900 mt-1 text-xs break-all">
                  {account.id}
                </p>
              </div>
              <div>
                <p className="text-slate-500">External Reference</p>
                <p className="font-mono text-slate-900 mt-1 text-xs break-all">
                  {account.external_reference_id || "--"}
                </p>
              </div>
              <div>
                <p className="text-slate-500">Parent Account</p>
                <p className="font-mono text-slate-900 mt-1 text-xs break-all">
                  {account.parent_id || "None (master)"}
                </p>
              </div>
              <div>
                <p className="text-slate-500">Created</p>
                <p className="font-medium text-slate-900 mt-1">
                  {account.created_at ? formatDate(account.created_at) : "--"}
                </p>
              </div>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
