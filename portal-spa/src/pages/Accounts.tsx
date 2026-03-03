import React, { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Wallet, Search, Eye, Building2 } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import { Pagination } from "../components/Pagination.tsx";
import type { BankAccountView, HouseAccountView } from "../types/index.ts";
import { formatBalance } from "../utils/currency.ts";

type Tab = "bank" | "house";

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
    CustomerClosed: "bg-slate-100 text-slate-600",
    active: "bg-[#DCFCE7] text-[#16A34A]"
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

const ACCOUNTS_PAGE_SIZE = 10;

function formatDate(dateStr: string): string {
  return new Date(dateStr).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric"
  });
}

function BankAccountsTab() {
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
          status_counts: Record<string, number>;
        }>(`/data/accounts?offset=${offset}&limit=${ACCOUNTS_PAGE_SIZE}`);
        return resp;
      } catch (err) {
        handleApiError(err);
        return {
          entries: [],
          pagination: { total: 0, offset: 0, limit: ACCOUNTS_PAGE_SIZE },
          status_counts: {}
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
  const statusCounts = accountData?.status_counts ?? {};

  const filtered = accounts.filter((a) => {
    if (!search) return true;
    const term = search.toLowerCase();
    return (
      a.account_number.toLowerCase().includes(term) ||
      (a.name ?? "").toLowerCase().includes(term) ||
      a.kind.toLowerCase().includes(term) ||
      a.currency.toLowerCase().includes(term) ||
      a.status.toLowerCase().includes(term)
    );
  });

  const totalCount = pagination.total;
  const activeCount = statusCounts["Approved"] ?? 0;
  const pendingCount = statusCounts["Pending"] ?? 0;
  const frozenCount = statusCounts["Freeze"] ?? 0;

  return (
    <>
      {/* Search */}
      <div className="flex justify-end mb-4">
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
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Name
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
                    <td className="px-6 text-[13px] text-slate-700">
                      {account.name || "\u2014"}
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
                        colSpan={7}
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
    </>
  );
}

function HouseAccountsTab() {
  const [search, setSearch] = useState("");

  const { data: houseData, isLoading } = useQuery({
    queryKey: ["house-accounts"],
    queryFn: async () => {
      try {
        return await api.get<{ entries: HouseAccountView[] }>(
          "/data/house-accounts"
        );
      } catch (err) {
        handleApiError(err);
        return { entries: [] };
      }
    }
  });

  const accounts = houseData?.entries ?? [];

  const filtered = accounts.filter((a) => {
    if (!search) return true;
    const term = search.toLowerCase();
    return (
      a.account_name.toLowerCase().includes(term) ||
      a.account_number.toLowerCase().includes(term) ||
      a.account_type.toLowerCase().includes(term) ||
      a.currency.toLowerCase().includes(term) ||
      a.status.toLowerCase().includes(term)
    );
  });

  return (
    <>
      {/* Search */}
      <div className="flex justify-end mb-4">
        <div className="relative">
          <Search className="absolute left-3.5 top-1/2 -translate-y-1/2 w-4 h-4 text-[#94A3B8]" />
          <input
            type="text"
            placeholder="Search house accounts..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            className="pl-10 pr-4 h-10 bg-white border border-slate-200 rounded-lg text-[13px] text-slate-900
              placeholder:text-[#94A3B8] focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent w-60"
          />
        </div>
      </div>

      {/* Stats */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <p className="text-xs font-medium text-slate-500">
            Total House Accounts
          </p>
          <p className="mt-1 text-[28px] font-bold font-mono text-slate-900">
            {isLoading ? "\u2014" : accounts.length}
          </p>
        </div>
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <p className="text-xs font-medium text-slate-500">Currencies</p>
          <p className="mt-1 text-[28px] font-bold font-mono text-slate-900">
            {isLoading
              ? "\u2014"
              : new Set(accounts.map((a) => a.currency)).size}
          </p>
        </div>
      </div>

      {/* Table */}
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
              </div>
            </div>
          ))}
        </div>
      ) : filtered.length === 0 ? (
        <div className="bg-white rounded-xl border border-slate-200 p-12 text-center">
          <Building2 className="w-10 h-10 text-slate-300 mx-auto mb-3" />
          <p className="text-slate-500 mb-1">
            {search
              ? "No house accounts match your search"
              : "No house accounts found"}
          </p>
          <p className="text-sm text-slate-400">
            {search
              ? "Try adjusting your search term."
              : "House accounts will appear here once created via the API."}
          </p>
        </div>
      ) : (
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <table className="w-full" aria-label="House accounts">
            <thead>
              <tr className="bg-[#F8FAFC] h-12">
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Account Number
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Name
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[120px]">
                  Type
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[80px]">
                  Currency
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[100px]">
                  Status
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Ledger ID
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {filtered.map((account) => (
                <tr key={account.id} className="hover:bg-slate-50 h-14">
                  <td className="px-6 text-[13px] font-mono font-medium text-slate-900">
                    {account.account_number}
                  </td>
                  <td className="px-6 text-[13px] text-slate-700">
                    {account.account_name}
                  </td>
                  <td className="px-6 text-[13px] text-slate-900 w-[120px]">
                    {account.account_type}
                  </td>
                  <td className="px-6 font-mono text-xs font-semibold text-slate-500 w-[80px]">
                    {account.currency}
                  </td>
                  <td className="px-6 w-[100px]">
                    <StatusBadge status={account.status} />
                  </td>
                  <td className="px-6 font-mono text-xs text-slate-500 break-all">
                    {account.ledger_id}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

export function Accounts() {
  const [tab, setTab] = useState<Tab>("bank");

  return (
    <div>
      {/* Page header */}
      <div className="flex items-center justify-between mb-6">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-3">
            <Wallet className="w-6 h-6 text-cyan-400" />
            <h1 className="text-2xl font-bold text-slate-900">Accounts</h1>
          </div>
          <p className="text-sm text-slate-500">
            View and manage bank accounts for your organization.
          </p>
        </div>
      </div>

      {/* Tab switcher */}
      <div className="flex gap-1 mb-6 bg-slate-100 rounded-lg p-1 w-fit">
        <button
          onClick={() => setTab("bank")}
          className={`px-4 py-2 rounded-md text-sm font-medium transition-colors ${
            tab === "bank"
              ? "bg-white text-slate-900 shadow-sm"
              : "text-slate-500 hover:text-slate-700"
          }`}
        >
          Bank Accounts
        </button>
        <button
          onClick={() => setTab("house")}
          className={`px-4 py-2 rounded-md text-sm font-medium transition-colors ${
            tab === "house"
              ? "bg-white text-slate-900 shadow-sm"
              : "text-slate-500 hover:text-slate-700"
          }`}
        >
          House Accounts
        </button>
      </div>

      {tab === "bank" ? <BankAccountsTab /> : <HouseAccountsTab />}
    </div>
  );
}
