import { useQuery } from "@tanstack/react-query";
import { Key, BarChart3, Shield, Clock, AlertTriangle } from "lucide-react";
import { useAuth } from "../hooks/useAuth.ts";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import type {
  DashboardStats,
  ActivityEntry,
  RateLimitEntry
} from "../types/index.ts";
import type { LucideIcon } from "lucide-react";

const ACTION_LABELS: Record<string, string> = {
  "api_key.created": "API key created",
  "api_key.rotated": "API key rotated",
  "api_key.revoked": "API key revoked",
  "api_key.grace_expired": "API key grace period expired",
  "member.invited": "Member invited",
  "member.role_changed": "Member role changed",
  "member.removed": "Member removed",
  "member.invite_resent": "Invitation resent"
};

const RATE_LIMIT_ACTIONS = new Set([
  "api_key.grace_expired",
  "rate_limit.exceeded"
]);

function formatAction(action: string): string {
  return ACTION_LABELS[action] ?? action;
}

function formatTimeAgo(dateStr: string): string {
  const now = new Date();
  const date = new Date(dateStr);
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  return `${diffDay}d ago`;
}

function StatCard({
  label,
  value,
  icon: Icon,
  variant = "default"
}: {
  label: string;
  value: string | number;
  icon: LucideIcon;
  variant?: "default" | "amber";
}) {
  const iconColor = variant === "amber" ? "text-amber-500" : "text-cyan-400";
  return (
    <div className="bg-white rounded-xl border border-slate-200 p-6">
      <Icon className={`w-5 h-5 ${iconColor} mb-3`} />
      <p className="text-sm font-medium text-slate-500">{label}</p>
      <p className="mt-2 text-3xl font-semibold text-slate-900">{value}</p>
    </div>
  );
}

function RateLimitBar({ entry }: { entry: RateLimitEntry }) {
  const requests = entry.requests_24h ?? 0;
  const throttled = entry.throttled_24h ?? 0;
  const total = requests + throttled;
  const throttledPct = total > 0 ? Math.round((throttled / total) * 100) : 0;

  // Health: green = no throttling, amber = some throttled, red = heavily throttled
  const isHealthy = throttled === 0;
  const isCritical = throttledPct >= 25;
  const dotColor = isHealthy
    ? "bg-green-500"
    : isCritical
      ? "bg-red-500"
      : "bg-amber-500";
  const barColor = isHealthy
    ? "bg-green-400"
    : isCritical
      ? "bg-red-500"
      : "bg-amber-400";
  // Bar shows success rate (requests out of total)
  const successPct = total > 0 ? Math.round((requests / total) * 100) : 100;

  return (
    <div className="py-3 border-b border-slate-100 last:border-0">
      <div className="flex items-center justify-between mb-1.5">
        <div className="flex items-center gap-2 min-w-0">
          <span className={`w-2 h-2 rounded-full shrink-0 ${dotColor}`} />
          <p className="text-sm font-medium text-slate-900 truncate">
            {entry.key_name}
          </p>
        </div>
        <span className="text-xs shrink-0 ml-2 text-slate-500">
          <span className="font-semibold text-slate-700">
            {requests.toLocaleString()}
          </span>
          {" / "}
          {entry.sustained_per_min.toLocaleString()} req/min
        </span>
      </div>
      <div className="w-full h-2.5 bg-slate-200 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${barColor}`}
          style={{ width: `${successPct}%` }}
        />
      </div>
      {throttled > 0 && (
        <p
          className={`mt-1 text-xs ${isCritical ? "font-medium text-red-600" : "text-amber-600"}`}
        >
          {throttled.toLocaleString()} throttled &mdash; exceeded burst cap (
          {entry.limit}/req), sustained rate is{" "}
          {entry.sustained_per_min.toLocaleString()}/min
        </p>
      )}
      {total === 0 && (
        <p className="mt-1 text-xs text-slate-400">No traffic in 24h</p>
      )}
    </div>
  );
}

export function Dashboard() {
  const { organization } = useAuth();

  const { data: stats, isLoading } = useQuery({
    queryKey: ["dashboard-stats"],
    queryFn: async () => {
      try {
        return await api.get<DashboardStats>("/dashboard/stats");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const { data: activity } = useQuery({
    queryKey: ["dashboard-activity"],
    queryFn: async () => {
      try {
        return await api.get<ActivityEntry[]>("/dashboard/activity");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const { data: rateLimits } = useQuery({
    queryKey: ["dashboard-rate-limits"],
    queryFn: async () => {
      try {
        return await api.get<RateLimitEntry[]>("/dashboard/rate-limits");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  return (
    <div>
      {/* Page header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">Dashboard</h1>
          <p className="mt-1 text-sm text-slate-500">
            Welcome back{organization ? `, ${organization.name}` : ""}. Here's
            your organization overview.
          </p>
        </div>
        {organization && (
          <div className="flex items-center gap-2 px-3.5 py-2 bg-white rounded-lg border border-slate-200">
            <span className="text-sm font-medium text-slate-900">
              {organization.name}
            </span>
          </div>
        )}
      </div>

      {/* Stats cards — order: Keys, Calls, Throttled, Scopes */}
      {isLoading ? (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
          {[1, 2, 3, 4].map((i) => (
            <div
              key={i}
              className="bg-white rounded-xl border border-slate-200 p-6 animate-pulse"
            >
              <div className="h-5 w-5 bg-slate-200 rounded mb-3" />
              <div className="h-4 bg-slate-200 rounded w-24 mb-4" />
              <div className="h-8 bg-slate-200 rounded w-16" />
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-5">
          <StatCard
            label="Active API Keys"
            value={stats?.active_api_keys ?? 0}
            icon={Key}
          />
          <StatCard
            label="API Calls (24h)"
            value={stats?.total_requests_today?.toLocaleString() ?? "0"}
            icon={BarChart3}
          />
          <StatCard
            label="Throttled (24h)"
            value={stats?.throttled_today ?? 0}
            icon={AlertTriangle}
            variant="amber"
          />
          <StatCard
            label="Scopes Granted"
            value={stats?.scopes_granted ?? 0}
            icon={Shield}
          />
        </div>
      )}

      {/* Rate Limit Usage + Recent Activity — side-by-side */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-5 mt-8">
        {/* Rate Limit Usage */}
        <div className="bg-white rounded-xl border border-slate-200 p-6">
          <h2 className="text-base font-semibold text-slate-900 mb-4">
            Rate Limit Usage by Key
          </h2>
          {!rateLimits || rateLimits.length === 0 ? (
            <p className="text-sm text-slate-500 text-center py-4">
              No API keys with rate limit data
            </p>
          ) : (
            <div className="space-y-0">
              {rateLimits.map((entry) => (
                <RateLimitBar key={entry.key_id} entry={entry} />
              ))}
            </div>
          )}
        </div>

        {/* Recent Activity */}
        <div className="bg-white rounded-xl border border-slate-200 p-6">
          <h2 className="text-base font-semibold text-slate-900 mb-4">
            Recent Activity
          </h2>
          <div className="space-y-3">
            {!activity || activity.length === 0 ? (
              <p className="text-sm text-slate-500 text-center py-4">
                No recent activity to display
              </p>
            ) : (
              activity.map((entry) => {
                const isRateLimitEvent = RATE_LIMIT_ACTIONS.has(entry.action);
                return (
                  <div
                    key={entry.id}
                    className="flex items-start gap-3 py-2 border-b border-slate-100 last:border-0"
                  >
                    <Clock
                      className={`w-4 h-4 mt-0.5 shrink-0 ${isRateLimitEvent ? "text-amber-500" : "text-slate-400"}`}
                    />
                    <div className="flex-1 min-w-0">
                      <p
                        className={`text-sm ${isRateLimitEvent ? "text-amber-700 font-medium" : "text-slate-900"}`}
                      >
                        {formatAction(entry.action)}
                      </p>
                      {entry.resource_id && (
                        <p className="text-xs text-slate-500 font-mono truncate mt-0.5">
                          {entry.resource_type}: {entry.resource_id}
                        </p>
                      )}
                    </div>
                    <span className="text-xs text-slate-400 shrink-0">
                      {formatTimeAgo(entry.created_at)}
                    </span>
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
