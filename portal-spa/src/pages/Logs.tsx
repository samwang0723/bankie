import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search, ChevronDown, ChevronRight } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import type { ApiLogsResponse, ApiLogEntry } from "../types/index.ts";

const HTTP_METHODS = ["GET", "POST", "PUT", "DELETE", "PATCH"];

function StatusCodeBadge({ code }: { code: number }) {
  let color = "bg-slate-100 text-slate-600";
  if (code >= 200 && code < 300) color = "bg-green-50 text-green-700";
  else if (code >= 400 && code < 500) color = "bg-amber-50 text-amber-700";
  else if (code >= 500) color = "bg-red-50 text-red-700";

  return (
    <span className={`px-2 py-0.5 rounded-full text-xs font-medium font-mono ${color}`}>
      {code}
    </span>
  );
}

function MethodBadge({ method }: { method: string }) {
  const colors: Record<string, string> = {
    GET: "text-green-700",
    POST: "text-blue-700",
    PUT: "text-amber-700",
    DELETE: "text-red-700",
    PATCH: "text-purple-700",
  };
  return (
    <span className={`font-mono text-xs font-semibold ${colors[method] ?? "text-slate-700"}`}>
      {method}
    </span>
  );
}

export function Logs() {
  const [page, setPage] = useState(1);
  const [method, setMethod] = useState("");
  const [statusCode, setStatusCode] = useState("");
  const [pathFilter, setPathFilter] = useState("");
  const [expandedId, setExpandedId] = useState<number | null>(null);
  const perPage = 25;

  const { data, isLoading } = useQuery({
    queryKey: ["api-logs", page, method, statusCode, pathFilter],
    queryFn: async () => {
      try {
        const params = new URLSearchParams({
          page: String(page),
          per_page: String(perPage),
        });
        if (method) params.set("method", method);
        if (statusCode) params.set("status_code", statusCode);
        if (pathFilter) params.set("path", pathFilter);
        return await api.get<ApiLogsResponse>(`/logs?${params}`);
      } catch (err) {
        handleApiError(err);
      }
    },
  });

  const logs = data?.logs ?? [];
  const total = data?.total ?? 0;
  const totalPages = Math.ceil(total / perPage);

  const resetFilters = () => {
    setMethod("");
    setStatusCode("");
    setPathFilter("");
    setPage(1);
  };

  const hasFilters = method || statusCode || pathFilter;

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-slate-900">API Logs</h1>
        <p className="text-sm text-slate-500 mt-1">
          View and filter API request logs
        </p>
      </div>

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-3 mb-6">
        {/* Method filter */}
        <select
          value={method}
          onChange={(e) => {
            setMethod(e.target.value);
            setPage(1);
          }}
          className="px-3 py-2 border border-slate-200 rounded-lg text-sm bg-white focus:outline-none focus:ring-2 focus:ring-cyan-400"
        >
          <option value="">All Methods</option>
          {HTTP_METHODS.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </select>

        {/* Status code filter */}
        <input
          type="text"
          value={statusCode}
          onChange={(e) => {
            setStatusCode(e.target.value.replace(/\D/g, ""));
            setPage(1);
          }}
          placeholder="Status code"
          className="w-28 px-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
        />

        {/* Path filter */}
        <div className="relative flex-1 min-w-[200px]">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
          <input
            type="text"
            value={pathFilter}
            onChange={(e) => {
              setPathFilter(e.target.value);
              setPage(1);
            }}
            placeholder="Filter by path prefix..."
            className="w-full pl-9 pr-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
          />
        </div>

        {hasFilters && (
          <button
            onClick={resetFilters}
            className="px-3 py-2 text-xs text-slate-500 hover:text-slate-700 hover:bg-slate-100 rounded-lg transition-colors"
          >
            Clear filters
          </button>
        )}
      </div>

      {/* Results count */}
      <div className="flex items-center justify-between mb-3">
        <span className="text-xs text-slate-400">
          {total.toLocaleString()} log{total !== 1 ? "s" : ""} found
        </span>
      </div>

      {/* Table */}
      {isLoading ? (
        <div className="space-y-2">
          {[1, 2, 3, 4, 5].map((i) => (
            <div
              key={i}
              className="h-12 bg-slate-100 rounded-lg animate-pulse"
            />
          ))}
        </div>
      ) : logs.length === 0 ? (
        <div className="text-center py-16">
          <Search className="w-12 h-12 text-slate-300 mx-auto mb-4" />
          <h3 className="text-lg font-semibold text-slate-700 mb-1">
            No logs found
          </h3>
          <p className="text-sm text-slate-500">
            {hasFilters
              ? "Try adjusting your filters."
              : "API logs will appear here once requests are made."}
          </p>
        </div>
      ) : (
        <div className="bg-white border border-slate-200 rounded-xl overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-slate-400 border-b border-slate-100 bg-slate-50/50">
                <th className="px-4 py-3 font-medium w-8"></th>
                <th className="px-4 py-3 font-medium">Method</th>
                <th className="px-4 py-3 font-medium">Path</th>
                <th className="px-4 py-3 font-medium">Status</th>
                <th className="px-4 py-3 font-medium">Latency</th>
                <th className="px-4 py-3 font-medium">Timestamp</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((log) => (
                <LogRow
                  key={log.id}
                  log={log}
                  expanded={expandedId === log.id}
                  onToggle={() =>
                    setExpandedId(expandedId === log.id ? null : log.id)
                  }
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex justify-center items-center gap-3 mt-6">
          <button
            onClick={() => setPage((p) => Math.max(1, p - 1))}
            disabled={page === 1}
            className="px-3 py-1.5 text-sm rounded-lg bg-slate-100 text-slate-600 hover:bg-slate-200 disabled:opacity-50 transition-colors"
          >
            Previous
          </button>
          <span className="text-sm text-slate-500">
            Page {page} of {totalPages}
          </span>
          <button
            onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
            disabled={page === totalPages}
            className="px-3 py-1.5 text-sm rounded-lg bg-slate-100 text-slate-600 hover:bg-slate-200 disabled:opacity-50 transition-colors"
          >
            Next
          </button>
        </div>
      )}
    </div>
  );
}

function LogRow({
  log,
  expanded,
  onToggle,
}: {
  log: ApiLogEntry;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <>
      <tr
        className="border-b border-slate-50 hover:bg-slate-50/50 cursor-pointer"
        onClick={onToggle}
      >
        <td className="px-4 py-3">
          {expanded ? (
            <ChevronDown className="w-3.5 h-3.5 text-slate-400" />
          ) : (
            <ChevronRight className="w-3.5 h-3.5 text-slate-400" />
          )}
        </td>
        <td className="px-4 py-3">
          <MethodBadge method={log.method} />
        </td>
        <td className="px-4 py-3 font-mono text-xs text-slate-700 max-w-[300px] truncate">
          {log.path}
        </td>
        <td className="px-4 py-3">
          <StatusCodeBadge code={log.status_code} />
        </td>
        <td className="px-4 py-3 text-xs text-slate-500">
          {log.latency_ms != null ? `${log.latency_ms}ms` : "—"}
        </td>
        <td className="px-4 py-3 text-xs text-slate-400">
          {new Date(log.created_at).toLocaleString()}
        </td>
      </tr>
      {expanded && (
        <tr>
          <td colSpan={6} className="px-4 py-3 bg-slate-50/50">
            <div className="grid grid-cols-2 gap-4 text-xs">
              <div>
                <span className="font-medium text-slate-500">Full Path</span>
                <p className="font-mono text-slate-700 mt-0.5 break-all">
                  {log.path}
                </p>
              </div>
              <div>
                <span className="font-medium text-slate-500">Client IP</span>
                <p className="font-mono text-slate-700 mt-0.5">
                  {log.client_ip ?? "—"}
                </p>
              </div>
              <div>
                <span className="font-medium text-slate-500">Status Code</span>
                <p className="mt-0.5">
                  <StatusCodeBadge code={log.status_code} />
                </p>
              </div>
              <div>
                <span className="font-medium text-slate-500">Latency</span>
                <p className="font-mono text-slate-700 mt-0.5">
                  {log.latency_ms != null ? `${log.latency_ms}ms` : "N/A"}
                </p>
              </div>
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
