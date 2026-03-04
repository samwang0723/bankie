import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ShieldCheck } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import { Pagination } from "../components/Pagination.tsx";
import type { AuditLogsResponse } from "../types/index.ts";

const ACTION_OPTIONS = [
  "auth.login_success",
  "auth.login_failed",
  "auth.logout",
  "org.created",
  "org.updated",
  "api_key.created",
  "api_key.rotated",
  "api_key.revoked",
  "api_key.grace_expired",
  "member.invited",
  "member.role_changed",
  "member.removed",
  "member.invite_resent",
  "webhook.created",
  "webhook.updated",
  "webhook.deleted",
  "webhook.secret_rotated"
];

const ACTION_LABELS: Record<string, string> = {
  "auth.login_success": "Login",
  "auth.login_failed": "Login failed",
  "auth.logout": "Logout",
  "org.created": "Org created",
  "org.updated": "Org updated",
  "api_key.created": "API key created",
  "api_key.rotated": "API key rotated",
  "api_key.revoked": "API key revoked",
  "api_key.grace_expired": "Grace expired",
  "member.invited": "Member invited",
  "member.role_changed": "Role changed",
  "member.removed": "Member removed",
  "member.invite_resent": "Invite resent",
  "webhook.created": "Webhook created",
  "webhook.updated": "Webhook updated",
  "webhook.deleted": "Webhook deleted",
  "webhook.secret_rotated": "Secret rotated"
};

function ActionBadge({ action }: { action: string }) {
  const isAuth = action.startsWith("auth.");
  const isError = action.includes("failed");
  const isDelete =
    action.includes("revoked") ||
    action.includes("removed") ||
    action.includes("deleted");

  let color = "bg-slate-100 text-slate-600";
  if (isError) color = "bg-red-50 text-red-700";
  else if (isDelete) color = "bg-amber-50 text-amber-700";
  else if (isAuth) color = "bg-blue-50 text-blue-700";
  else color = "bg-green-50 text-green-700";

  return (
    <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${color}`}>
      {ACTION_LABELS[action] ?? action}
    </span>
  );
}

function ChangesCell({ changes }: { changes: Record<string, unknown> | null }) {
  if (!changes || Object.keys(changes).length === 0) {
    return <span className="text-xs text-slate-300">&mdash;</span>;
  }

  const entries = Object.entries(changes).slice(0, 3);
  return (
    <div className="space-y-0.5">
      {entries.map(([key, val]) => (
        <p key={key} className="text-xs text-slate-500 truncate max-w-[200px]">
          <span className="font-medium text-slate-600">{key}:</span>{" "}
          {typeof val === "string" ? val : JSON.stringify(val)}
        </p>
      ))}
      {Object.keys(changes).length > 3 && (
        <p className="text-xs text-slate-400">
          +{Object.keys(changes).length - 3} more
        </p>
      )}
    </div>
  );
}

export function AuditLogs() {
  const [offset, setOffset] = useState(0);
  const [action, setAction] = useState("");
  const [fromDate, setFromDate] = useState("");
  const [toDate, setToDate] = useState("");
  const limit = 25;

  const { data, isLoading } = useQuery({
    queryKey: ["audit-logs", offset, action, fromDate, toDate],
    queryFn: async () => {
      try {
        const params = new URLSearchParams({
          offset: String(offset),
          limit: String(limit)
        });
        if (action) params.set("action", action);
        if (fromDate) params.set("from", fromDate);
        if (toDate) params.set("to", toDate);
        return await api.get<AuditLogsResponse>(`/audit-logs?${params}`);
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const logs = data?.data ?? [];
  const total = data?.total ?? 0;

  const hasFilters = action || fromDate || toDate;

  const resetFilters = () => {
    setAction("");
    setFromDate("");
    setToDate("");
    setOffset(0);
  };

  return (
    <div>
      <div className="mb-8">
        <h1 className="text-2xl font-bold text-slate-900">Audit Logs</h1>
        <p className="text-sm text-slate-500 mt-1">
          Track all actions and changes in your organization
        </p>
      </div>

      {/* Filters */}
      <div className="flex flex-wrap items-center gap-3 mb-6">
        <select
          value={action}
          onChange={(e) => {
            setAction(e.target.value);
            setOffset(0);
          }}
          className="px-3 py-2 border border-slate-200 rounded-lg text-sm bg-white focus:outline-none focus:ring-2 focus:ring-cyan-400"
        >
          <option value="">All Actions</option>
          {ACTION_OPTIONS.map((a) => (
            <option key={a} value={a}>
              {ACTION_LABELS[a] ?? a}
            </option>
          ))}
        </select>

        <div className="flex items-center gap-2">
          <label className="text-xs text-slate-500">From</label>
          <input
            type="date"
            value={fromDate}
            onChange={(e) => {
              setFromDate(e.target.value);
              setOffset(0);
            }}
            className="px-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
          />
        </div>

        <div className="flex items-center gap-2">
          <label className="text-xs text-slate-500">To</label>
          <input
            type="date"
            value={toDate}
            onChange={(e) => {
              setToDate(e.target.value);
              setOffset(0);
            }}
            className="px-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
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
          {total.toLocaleString()} event{total !== 1 ? "s" : ""} found
        </span>
      </div>

      {/* Table */}
      {isLoading ? (
        <div className="space-y-2">
          {[1, 2, 3, 4, 5].map((i) => (
            <div
              key={i}
              className="h-14 bg-slate-100 rounded-lg animate-pulse"
            />
          ))}
        </div>
      ) : logs.length === 0 ? (
        <div className="text-center py-16">
          <ShieldCheck className="w-12 h-12 text-slate-300 mx-auto mb-4" />
          <h3 className="text-lg font-semibold text-slate-700 mb-1">
            No audit events found
          </h3>
          <p className="text-sm text-slate-500">
            {hasFilters
              ? "Try adjusting your filters."
              : "Audit events will appear here as actions are performed."}
          </p>
        </div>
      ) : (
        <div className="bg-white border border-slate-200 rounded-xl overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-slate-400 border-b border-slate-100 bg-slate-50/50">
                <th className="px-4 py-3 font-medium">Action</th>
                <th className="px-4 py-3 font-medium">Resource</th>
                <th className="px-4 py-3 font-medium">Changes</th>
                <th className="px-4 py-3 font-medium">IP Address</th>
                <th className="px-4 py-3 font-medium">Timestamp</th>
              </tr>
            </thead>
            <tbody>
              {logs.map((log) => (
                <tr
                  key={log.id}
                  className="border-b border-slate-50 hover:bg-slate-50/50"
                >
                  <td className="px-4 py-3">
                    <ActionBadge action={log.action} />
                  </td>
                  <td className="px-4 py-3">
                    <p className="text-xs text-slate-600">
                      {log.resource_type}
                    </p>
                    {log.resource_id && (
                      <p className="text-xs text-slate-400 font-mono truncate max-w-[180px]">
                        {log.resource_id}
                      </p>
                    )}
                  </td>
                  <td className="px-4 py-3">
                    <ChangesCell changes={log.changes} />
                  </td>
                  <td className="px-4 py-3 text-xs text-slate-500 font-mono">
                    {log.client_ip ?? "—"}
                  </td>
                  <td className="px-4 py-3 text-xs text-slate-400">
                    {new Date(log.created_at).toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          <Pagination
            offset={offset}
            limit={limit}
            total={total}
            onPageChange={setOffset}
          />
        </div>
      )}
    </div>
  );
}
