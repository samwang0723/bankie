import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeftRight, Download, Calendar } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import { Pagination } from "../components/Pagination.tsx";
import type { Transaction, BankAccountView } from "../types/index.ts";

function formatAccountNumber(raw: string): string {
  const digits = raw.replace(/\D/g, "");
  if (digits.length !== 12) return raw;
  return `${digits.slice(0, 4)}-${digits.slice(4, 8)}-${digits.slice(8, 12)}`;
}

const TYPE_LABELS: Record<string, string> = {
  deposit: "Deposit",
  withdrawal: "Withdrawal",
  transfer: "Transfer"
};

const STATUS_LABELS: Record<string, string> = {
  completed: "Completed",
  processing: "Pending",
  failed: "Failed"
};

function TypeBadge({ txType }: { txType: string }) {
  const label = TYPE_LABELS[txType] ?? txType;
  const styles: Record<string, string> = {
    Deposit: "bg-[#DCFCE7] text-[#16A34A]",
    Withdrawal: "bg-[#FEE2E2] text-[#DC2626]",
    Transfer: "bg-[#E0E7FF] text-[#4F46E5]"
  };
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded text-[11px] font-semibold ${
        styles[label] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {label}
    </span>
  );
}

function TxStatusBadge({ status }: { status: string }) {
  const label = STATUS_LABELS[status] ?? status;
  const styles: Record<string, string> = {
    Completed: "bg-[#DCFCE7] text-[#16A34A]",
    Pending: "bg-[#FEF3C7] text-[#D97706]",
    Failed: "bg-[#FEE2E2] text-[#DC2626]"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-[11px] font-semibold ${
        styles[label] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {label}
    </span>
  );
}

function formatDateTime(dateStr: string): string {
  const d = new Date(dateStr);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  const h = String(d.getHours()).padStart(2, "0");
  const min = String(d.getMinutes()).padStart(2, "0");
  return `${y}-${m}-${day} ${h}:${min}`;
}

function formatAmount(
  amount: string,
  txType: string,
  currency: string
): string {
  const num = parseFloat(amount);
  const prefix = txType === "withdrawal" ? "- " : "+ ";
  const isFiat = currency === "USD" || currency === "TWD";
  if (isFiat) {
    return `${prefix}$${Math.abs(num).toLocaleString("en-US", {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2
    })}`;
  }
  return `${prefix}${Math.abs(num).toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 8
  })}`;
}

function downloadReport(params: {
  start_date: string;
  end_date: string;
  currency?: string;
}) {
  const searchParams = new URLSearchParams();
  searchParams.set("start_date", params.start_date);
  searchParams.set("end_date", params.end_date);
  if (params.currency && params.currency !== "all") {
    searchParams.set("currency", params.currency);
  }
  window.open(
    `/api/portal/v1/data/reports/settlement?${searchParams}`,
    "_blank"
  );
}

function todayStr(): string {
  return new Date().toISOString().split("T")[0];
}

function daysAgoStr(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return d.toISOString().split("T")[0];
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

const PAGE_SIZE = 10;

export function Transactions() {
  const [startDate, setStartDate] = useState(() => daysAgoStr(5));
  const [endDate, setEndDate] = useState(todayStr);
  const [typeFilter, setTypeFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");
  const [offset, setOffset] = useState(0);

  // Fetch all accounts for balance summary + account number lookup
  const { data: accountsData } = useQuery({
    queryKey: ["accounts-for-tx"],
    queryFn: async () => {
      try {
        const resp = await api.get<{ entries: BankAccountView[] }>(
          "/data/accounts?offset=0&limit=100"
        );
        return resp.entries;
      } catch (err) {
        handleApiError(err);
        return [];
      }
    }
  });

  const allAccounts = accountsData ?? [];

  // Aggregate balances across ALL accounts for summary cards
  const balanceSummary = useMemo(() => {
    if (allAccounts.length === 0) return null;
    let totalAvailable = 0;
    let totalPending = 0;
    let totalBookBalance = 0;
    const currencies = new Set<string>();
    for (const a of allAccounts) {
      currencies.add(a.currency);
      const av = parseFloat(a.available ?? "0");
      const pn = parseFloat(a.pending ?? "0");
      const bb = parseFloat(a.book_balance ?? "0");
      if (!isNaN(av)) totalAvailable += av;
      if (!isNaN(pn)) totalPending += pn;
      if (!isNaN(bb)) totalBookBalance += bb;
    }
    const currency = currencies.size === 1 ? [...currencies][0] : "USD";
    return { totalAvailable, totalPending, totalBookBalance, currency };
  }, [allAccounts]);

  // Build account ID → account_number lookup map
  const accountNumberMap = useMemo(() => {
    const map = new Map<string, string>();
    for (const a of allAccounts) {
      map.set(a.id, a.account_number);
    }
    return map;
  }, [allAccounts]);

  // Build query params for transactions
  const queryParams = new URLSearchParams();
  queryParams.set("offset", String(offset));
  queryParams.set("limit", String(PAGE_SIZE));
  if (startDate) queryParams.set("start_date", startDate);
  if (endDate) queryParams.set("end_date", endDate);
  if (typeFilter !== "all") queryParams.set("transaction_type", typeFilter);
  if (statusFilter !== "all") queryParams.set("status", statusFilter);

  const { data: txData, isLoading } = useQuery({
    queryKey: [
      "transactions",
      startDate,
      endDate,
      typeFilter,
      statusFilter,
      offset
    ],
    queryFn: async () => {
      try {
        const resp = await api.get<{
          entries: Transaction[];
          pagination: { total: number; offset: number; limit: number };
        }>(`/data/transactions?${queryParams}`);
        return resp;
      } catch (err) {
        handleApiError(err);
        return {
          entries: [],
          pagination: { total: 0, offset: 0, limit: PAGE_SIZE }
        };
      }
    }
  });

  const transactions = txData?.entries ?? [];
  const pagination = txData?.pagination ?? {
    total: 0,
    offset: 0,
    limit: PAGE_SIZE
  };

  function handleExportCsv() {
    const today = new Date();
    const thirtyDaysAgo = new Date(today);
    thirtyDaysAgo.setDate(today.getDate() - 30);

    downloadReport({
      start_date: startDate || thirtyDaysAgo.toISOString().split("T")[0],
      end_date: endDate || today.toISOString().split("T")[0]
    });
  }

  return (
    <div>
      {/* Page header */}
      <div className="flex items-center justify-between mb-8">
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-3">
            <ArrowLeftRight className="w-6 h-6 text-cyan-400" />
            <h1 className="text-2xl font-bold text-slate-900">Transactions</h1>
          </div>
          <p className="text-sm text-slate-500">
            View transaction history and ledger balances.
          </p>
        </div>
        <button
          onClick={handleExportCsv}
          className="flex items-center gap-2 h-10 px-4 bg-white border border-slate-200 rounded-lg
            text-[13px] font-medium text-slate-900 hover:bg-slate-50 transition-colors"
        >
          <Download className="w-4 h-4 text-slate-500" />
          Export CSV
        </button>
      </div>

      {/* Filter row */}
      <div className="flex flex-wrap items-center gap-3 mb-6">
        <div className="flex items-center gap-2 h-9 px-3.5 bg-white border border-slate-200 rounded-lg">
          <Calendar className="w-3.5 h-3.5 text-slate-500" />
          <input
            type="date"
            value={startDate}
            onChange={(e) => {
              setStartDate(e.target.value);
              setOffset(0);
            }}
            className="text-xs font-medium text-slate-900 bg-transparent border-none outline-none"
          />
          <span className="text-slate-400">-</span>
          <input
            type="date"
            value={endDate}
            onChange={(e) => {
              setEndDate(e.target.value);
              setOffset(0);
            }}
            className="text-xs font-medium text-slate-900 bg-transparent border-none outline-none"
          />
        </div>
        <select
          value={typeFilter}
          onChange={(e) => {
            setTypeFilter(e.target.value);
            setOffset(0);
          }}
          className="h-9 px-3.5 bg-white border border-slate-200 rounded-lg text-xs font-medium text-slate-500
            focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
        >
          <option value="all">All Types</option>
          <option value="deposit">Deposit</option>
          <option value="withdrawal">Withdrawal</option>
          <option value="transfer">Transfer</option>
        </select>
        <select
          value={statusFilter}
          onChange={(e) => {
            setStatusFilter(e.target.value);
            setOffset(0);
          }}
          className="h-9 px-3.5 bg-white border border-slate-200 rounded-lg text-xs font-medium text-slate-500
            focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
        >
          <option value="all">All Statuses</option>
          <option value="completed">Completed</option>
          <option value="processing">Pending</option>
          <option value="failed">Failed</option>
        </select>
      </div>

      {/* Ledger balance cards — aggregated across all accounts */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center justify-between mb-2">
            <p className="text-xs font-medium text-slate-500">Available</p>
            <span className="font-mono text-[11px] font-semibold text-[#94A3B8]">
              {balanceSummary?.currency ?? ""}
            </span>
          </div>
          <p className="text-2xl font-bold font-mono text-slate-900">
            {formatBalance(
              balanceSummary?.totalAvailable.toString(),
              balanceSummary?.currency ?? "USD"
            )}
          </p>
        </div>
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center justify-between mb-2">
            <p className="text-xs font-medium text-slate-500">Pending</p>
            <span className="font-mono text-[11px] font-semibold text-[#94A3B8]">
              {balanceSummary?.currency ?? ""}
            </span>
          </div>
          <p className="text-2xl font-bold font-mono text-[#F59E0B]">
            {formatBalance(
              balanceSummary?.totalPending.toString(),
              balanceSummary?.currency ?? "USD"
            )}
          </p>
        </div>
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center justify-between mb-2">
            <p className="text-xs font-medium text-slate-500">Current</p>
            <span className="font-mono text-[11px] font-semibold text-[#94A3B8]">
              {balanceSummary?.currency ?? ""}
            </span>
          </div>
          <p className="text-2xl font-bold font-mono text-slate-900">
            {formatBalance(
              balanceSummary?.totalBookBalance.toString(),
              balanceSummary?.currency ?? "USD"
            )}
          </p>
        </div>
      </div>

      {/* Transactions table */}
      {isLoading ? (
        <div className="bg-white rounded-xl border border-slate-200">
          {[1, 2, 3, 4, 5].map((i) => (
            <div
              key={i}
              className="p-4 border-b border-slate-100 animate-pulse"
            >
              <div className="flex items-center gap-4">
                <div className="h-4 bg-slate-200 rounded w-28" />
                <div className="h-4 bg-slate-200 rounded w-24" />
                <div className="h-4 bg-slate-200 rounded w-16" />
                <div className="h-4 bg-slate-200 rounded w-20" />
                <div className="h-4 bg-slate-200 rounded w-16" />
              </div>
            </div>
          ))}
        </div>
      ) : transactions.length === 0 ? (
        <div className="bg-white rounded-xl border border-slate-200 p-12 text-center">
          <ArrowLeftRight className="w-10 h-10 text-slate-300 mx-auto mb-3" />
          <p className="text-slate-500 mb-1">No transactions found</p>
          <p className="text-sm text-slate-400">
            Transactions will appear here after deposits, withdrawals, or
            transfers.
          </p>
        </div>
      ) : (
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <table className="w-full" aria-label="Transactions">
            <thead>
              <tr className="bg-[#F8FAFC] h-11">
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[160px]">
                  Date
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Account
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[100px]">
                  Type
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[80px]">
                  Currency
                </th>
                <th className="text-right px-6 text-xs font-semibold text-slate-500 w-[170px]">
                  Amount
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500 w-[100px]">
                  Status
                </th>
                <th className="text-left px-6 text-xs font-semibold text-slate-500">
                  Reference
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {transactions.map((tx) => (
                <tr key={tx.id} className="hover:bg-slate-50 h-[52px]">
                  <td className="px-6 font-mono text-xs font-medium text-slate-900 w-[160px] whitespace-nowrap">
                    {formatDateTime(tx.transaction_date)}
                  </td>
                  <td className="px-6 font-mono text-xs font-medium text-slate-900">
                    {formatAccountNumber(
                      accountNumberMap.get(tx.bank_account_id) ??
                        tx.bank_account_id.substring(0, 14)
                    )}
                  </td>
                  <td className="px-6 w-[100px]">
                    <TypeBadge txType={tx.transaction_type} />
                  </td>
                  <td className="px-6 font-mono text-xs font-semibold text-slate-500 w-[80px]">
                    {tx.currency}
                  </td>
                  <td
                    className={`px-6 font-mono text-[13px] text-right font-semibold w-[170px] whitespace-nowrap ${
                      tx.transaction_type === "withdrawal"
                        ? "text-[#DC2626]"
                        : "text-[#16A34A]"
                    }`}
                  >
                    {formatAmount(tx.amount, tx.transaction_type, tx.currency)}
                  </td>
                  <td className="px-6 w-[100px]">
                    <TxStatusBadge status={tx.status} />
                  </td>
                  <td className="px-6 font-mono text-[11px] font-medium text-[#94A3B8]">
                    {tx.transaction_reference
                      ? `${tx.transaction_reference.substring(0, 12)}...`
                      : "--"}
                  </td>
                </tr>
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
