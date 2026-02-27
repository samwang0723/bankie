import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  ArrowLeftRight,
  Download,
  DollarSign,
  Clock,
  TrendingUp
} from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import type { Transaction, BankAccountView } from "../types/index.ts";

const TYPE_LABELS: Record<string, string> = {
  deposit: "Deposit",
  withdrawal: "Withdrawal",
  transfer: "Transfer"
};

const STATUS_LABELS: Record<string, string> = {
  posted: "Completed",
  pending: "Pending",
  failed: "Failed"
};

function TypeBadge({ txType }: { txType: string }) {
  const label = TYPE_LABELS[txType] ?? txType;
  const styles: Record<string, string> = {
    Deposit: "bg-green-50 text-green-700",
    Withdrawal: "bg-red-50 text-red-700",
    Transfer: "bg-cyan-50 text-cyan-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
        styles[label] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {label}
    </span>
  );
}

function StatusBadge({ status }: { status: string }) {
  const label = STATUS_LABELS[status] ?? status;
  const styles: Record<string, string> = {
    Completed: "bg-green-50 text-green-700",
    Pending: "bg-amber-50 text-amber-700",
    Failed: "bg-red-50 text-red-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${
        styles[label] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {label}
    </span>
  );
}

function formatDateTime(dateStr: string): string {
  return new Date(dateStr).toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit"
  });
}

function formatAmount(amount: string, txType: string): string {
  const num = parseFloat(amount);
  const prefix = txType === "withdrawal" ? "-" : "+";
  return `${prefix}${Math.abs(num).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 8 })}`;
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

export function Transactions() {
  const [startDate, setStartDate] = useState(todayStr);
  const [endDate, setEndDate] = useState(todayStr);
  const [typeFilter, setTypeFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");

  // Fetch accounts and pick the first active (Approved) one for balance summary
  const { data: firstAccount } = useQuery({
    queryKey: ["accounts-for-balance"],
    queryFn: async () => {
      try {
        const resp = await api.get<{ entries: BankAccountView[] }>(
          "/data/accounts?offset=0&limit=100"
        );
        const active = resp.entries.find((a) => a.status === "Approved");
        return active ?? (resp.entries.length > 0 ? resp.entries[0] : null);
      } catch (err) {
        handleApiError(err);
        return null;
      }
    }
  });

  // Build query params for transactions
  const queryParams = new URLSearchParams();
  queryParams.set("offset", "0");
  queryParams.set("limit", "50");
  if (startDate) queryParams.set("start_date", startDate);
  if (endDate) queryParams.set("end_date", endDate);
  if (typeFilter !== "all") queryParams.set("transaction_type", typeFilter);
  if (statusFilter !== "all") queryParams.set("status", statusFilter);

  const { data: transactions = [], isLoading } = useQuery({
    queryKey: ["transactions", startDate, endDate, typeFilter, statusFilter],
    queryFn: async () => {
      try {
        const resp = await api.get<{ entries: Transaction[] }>(
          `/data/transactions?${queryParams}`
        );
        return resp.entries;
      } catch (err) {
        handleApiError(err);
        return [];
      }
    }
  });

  function handleExportCsv() {
    const today = new Date();
    const thirtyDaysAgo = new Date(today);
    thirtyDaysAgo.setDate(today.getDate() - 30);

    downloadReport({
      start_date: startDate || thirtyDaysAgo.toISOString().split("T")[0],
      end_date: endDate || today.toISOString().split("T")[0]
    });
  }

  function formatBalance(value: string | undefined): string {
    if (value == null) return "--";
    const num = parseFloat(value);
    if (isNaN(num)) return "--";
    return num.toLocaleString("en-US", { minimumFractionDigits: 2 });
  }

  return (
    <div>
      {/* Page header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <div className="flex items-center gap-3 mb-1">
            <ArrowLeftRight className="w-6 h-6 text-cyan-400" />
            <h1 className="text-2xl font-bold text-slate-900">Transactions</h1>
          </div>
          <p className="mt-1 text-sm text-slate-500">
            View transaction history and ledger balances.
          </p>
        </div>
        <button
          onClick={handleExportCsv}
          className="flex items-center gap-2 h-10 px-5 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
            hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2"
        >
          <Download className="w-4 h-4" />
          Export CSV
        </button>
      </div>

      {/* Filter row */}
      <div className="bg-white rounded-xl border border-slate-200 p-4 mb-6">
        <div className="flex flex-wrap items-end gap-4">
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">
              Start Date
            </label>
            <input
              type="date"
              value={startDate}
              onChange={(e) => setStartDate(e.target.value)}
              className="h-9 px-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">
              End Date
            </label>
            <input
              type="date"
              value={endDate}
              onChange={(e) => setEndDate(e.target.value)}
              className="h-9 px-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">
              Type
            </label>
            <select
              value={typeFilter}
              onChange={(e) => setTypeFilter(e.target.value)}
              className="h-9 px-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
            >
              <option value="all">All</option>
              <option value="deposit">Deposit</option>
              <option value="withdrawal">Withdrawal</option>
              <option value="transfer">Transfer</option>
            </select>
          </div>
          <div>
            <label className="block text-xs font-medium text-slate-500 mb-1">
              Status
            </label>
            <select
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value)}
              className="h-9 px-3 bg-white border border-slate-200 rounded-lg text-sm text-slate-900
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-transparent"
            >
              <option value="all">All</option>
              <option value="posted">Completed</option>
              <option value="pending">Pending</option>
              <option value="failed">Failed</option>
            </select>
          </div>
        </div>
      </div>

      {/* Balance cards — uses inline balances from accounts endpoint */}
      <div className="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center gap-2 mb-2">
            <DollarSign className="w-4 h-4 text-green-500" />
            <p className="text-sm font-medium text-slate-500">Available</p>
          </div>
          <p className="text-2xl font-semibold text-slate-900">
            {formatBalance(firstAccount?.available)}
          </p>
          {firstAccount && (
            <p className="text-xs text-slate-400 mt-1">
              {firstAccount.currency}
            </p>
          )}
        </div>
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center gap-2 mb-2">
            <Clock className="w-4 h-4 text-amber-500" />
            <p className="text-sm font-medium text-slate-500">Pending</p>
          </div>
          <p className="text-2xl font-semibold text-amber-600">
            {formatBalance(firstAccount?.pending)}
          </p>
          {firstAccount && (
            <p className="text-xs text-slate-400 mt-1">
              {firstAccount.currency}
            </p>
          )}
        </div>
        <div className="bg-white rounded-xl border border-slate-200 p-5">
          <div className="flex items-center gap-2 mb-2">
            <TrendingUp className="w-4 h-4 text-cyan-500" />
            <p className="text-sm font-medium text-slate-500">Balance</p>
          </div>
          <p className="text-2xl font-semibold text-slate-900">
            {formatBalance(firstAccount?.book_balance)}
          </p>
          {firstAccount && (
            <p className="text-xs text-slate-400 mt-1">
              {firstAccount.currency}
            </p>
          )}
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
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Date
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Account
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Type
                </th>
                <th className="text-right px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Amount
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Status
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Reference
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {transactions.map((tx) => (
                <tr key={tx.id} className="hover:bg-slate-50 h-14">
                  <td className="px-6 py-3 text-sm text-slate-600">
                    {formatDateTime(tx.transaction_date)}
                  </td>
                  <td className="px-6 py-3 text-sm font-mono text-slate-900 text-xs">
                    {tx.bank_account_id.substring(0, 8)}...
                  </td>
                  <td className="px-6 py-3">
                    <TypeBadge txType={tx.transaction_type} />
                  </td>
                  <td
                    className={`px-6 py-3 text-sm font-mono text-right font-medium ${
                      tx.transaction_type === "withdrawal"
                        ? "text-red-600"
                        : "text-green-600"
                    }`}
                  >
                    {formatAmount(tx.amount, tx.transaction_type)} {tx.currency}
                  </td>
                  <td className="px-6 py-3">
                    <StatusBadge status={tx.status} />
                  </td>
                  <td className="px-6 py-3 text-sm font-mono text-slate-500 text-xs">
                    {tx.transaction_reference
                      ? `${tx.transaction_reference.substring(0, 12)}...`
                      : "--"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
