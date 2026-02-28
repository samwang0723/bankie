import React, { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Wallet, Search, Eye } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import { Pagination } from "../components/Pagination.tsx";
import type { BankAccountView } from "../types/index.ts";

function formatAccountNumber(raw: string): string {
  const digits = raw.replace(/\D/g, "");
  if (digits.length !== 12) return raw;
  return `${digits.slice(0, 4)}-${digits.slice(4, 8)}-${digits.slice(8, 12)}`;
}

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    Approved: "bg-[#DCFCE7] text-[#16A34A]",
    Pending: "bg-[#FEF3C7] text-[#D97706]",
    Freeze: "bg-red-50 text-red-700",
    CustomerClosed: "bg-slate-100 text-slate-600"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-[11px] font-semibold ${
        styles[status] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {status}
    </span>
  );
}

function formatBalance(value: string | undefined, currency: string): string {
  if (value == null) return "--";
  const num = parseFloat(value);
  if (isNaN(num)) return "--";
  const isFiat = currency === "USD" || currency === "TWD";
  if (isFiat) {
    return `$${num.toLocaleString("en-US", { minimumFractionDigits: 2 })}`;
  }
  return num.toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 8
  });
}

const ACCOUNTS_PAGE_SIZE = 10;

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
  const [offset, setOffset] = useState(0);

  const { data: accountData, isLoading } = useQuery({
    queryKey: ["accounts", offset],
    queryFn: async () => {
      try {
        const resp = await api.get<{
          entries: BankAccountView[];
          pagination: { total: number; offset: number; limit: number };
        }>(`/data/accounts?offset=${offset}&limit=${ACCOUNTS_PAGE_SIZE}`);
        return resp;
      } catch (err) {
        handleApiError(err);
        return {
          entries: [],
          pagination: { total: 0, offset: 0, limit: ACCOUNTS_PAGE_SIZE }
        };
      }
    }
  });

  const accounts = accountData?.entries ?? [];
  const pagination = accountData?.pagination ?? {
    total: 0,
    offset: 0,
    limit: ACCOUNTS_PAGE_SIZE
  };

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

  const totalCount = pagination.total;
  const activeCount = accounts.filter((a) => a.status === "Approved").length;
  const pendingCount = accounts.filter((a) => a.status === "Pending").length;
  const frozenCount = accounts.filter((a) => a.status === "Freeze").length;

  return (
    <div>
      {/* Page header */}
      <div className="flex items-center justify-between mb-8">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-3">
            <Wallet className="w-6 h-6 text-cyan-400" />
            <h1 className="text-2xl font-bold text-slate-900">Accounts</h1>
          </div>
          <p className="text-sm text-slate-500">
            View and manage bank accounts for your organization.
          </p>
        </div>
        <div className="relative">
          <Search className="absolute left-3.5 top-1/2 -translate-y-1/2 w-4 h-4 text-[#94A3B8]" />
          <input
            type="text"
            placeholder="Search accounts..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-10 pr-4 h-10 bg-white border border-slate-200 rounded-lg text-[13px] text-slate-900
              placeholder:text-[#94A3B8] focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent w-60"
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
            <p className="text-xs font-medium text-slate-500">Total Accounts</p>
            <p className="mt-1 text-[28px] font-bold font-mono text-slate-900">
              {totalCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-xs font-medium text-slate-500">Active</p>
            <p className="mt-1 text-[28px] font-bold font-mono text-[#16A34A]">
              {activeCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-xs font-medium text-slate-500">Pending</p>
            <p className="mt-1 text-[28px] font-bold font-mono text-[#F59E0B]">
              {pendingCount}
            </p>
          </div>
          <div className="bg-white rounded-xl border border-slate-200 p-5">
            <p className="text-xs font-medium text-slate-500">Frozen</p>
            <p className="mt-1 text-[28px] font-bold font-mono text-[#DC2626]">
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
              <tr className="bg-[#F8FAFC] h-12">
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Account Number
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[100px]">
                  Type
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[80px]">
                  Currency
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[100px]">
                  Status
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Balance
                </th>
                <th className="text-right px-6 text-xs font-semibold text-slate-500 w-[80px]">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {filtered.map((account) => (
                <React.Fragment key={account.id}>
                  <tr className="hover:bg-slate-50 h-14">
                    <td className="px-6 text-[13px] font-mono font-medium text-slate-900">
                      {formatAccountNumber(account.account_number)}
                    </td>
                    <td className="px-6 text-[13px] text-slate-900 w-[100px]">
                      {account.kind}
                    </td>
                    <td className="px-6 font-mono text-xs font-semibold text-slate-500 w-[80px]">
                      {account.currency}
                    </td>
                    <td className="px-6 w-[100px]">
                      <StatusBadge status={account.status} />
                    </td>
                    <td className="px-6 font-mono text-[13px] font-semibold text-slate-900">
                      {formatBalance(account.available, account.currency)}
                    </td>
                    <td className="px-6 text-right w-[80px]">
                      <button
                        onClick={() =>
                          setExpandedId(
                            expandedId === account.id ? null : account.id
                          )
                        }
                        className="p-1.5 text-[#94A3B8] hover:text-cyan-500 transition-colors"
                        aria-label={`View details for ${account.account_number}`}
                      >
                        <Eye className="w-4 h-4" />
                      </button>
                    </td>
                  </tr>
                  {expandedId === account.id && (
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
                              {account.created_at
                                ? formatDate(account.created_at)
                                : "--"}
                            </p>
                          </div>
                        </div>
                      </td>
                    </tr>
                  )}
                </React.Fragment>
              ))}
            </tbody>
          </table>
          <Pagination
            offset={pagination.offset}
            limit={pagination.limit}
            total={pagination.total}
            onPageChange={setOffset}
          />
        </div>
      )}
    </div>
  );
}
