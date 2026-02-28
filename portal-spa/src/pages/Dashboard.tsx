import { useQuery } from '@tanstack/react-query';
import { Key, BarChart3, Shield, Clock } from 'lucide-react';
import { useAuth } from '../hooks/useAuth.ts';
import { api } from '../api/client.ts';
import { handleApiError } from '../hooks/useAuth.ts';
import type { DashboardStats, ActivityEntry } from '../types/index.ts';
import type { LucideIcon } from 'lucide-react';

const ACTION_LABELS: Record<string, string> = {
  'api_key.created': 'API key created',
  'api_key.rotated': 'API key rotated',
  'api_key.revoked': 'API key revoked',
};

function formatAction(action: string): string {
  return ACTION_LABELS[action] ?? action;
}

function formatTimeAgo(dateStr: string): string {
  const now = new Date();
  const date = new Date(dateStr);
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / 60000);
  if (diffMin < 1) return 'just now';
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
}: {
  label: string;
  value: string | number;
  icon: LucideIcon;
}) {
  return (
    <div className="bg-white rounded-xl border border-slate-200 p-6">
      <Icon className="w-5 h-5 text-cyan-400 mb-3" />
      <p className="text-sm font-medium text-slate-500">{label}</p>
      <p className="mt-2 text-3xl font-semibold text-slate-900">{value}</p>
    </div>
  );
}

export function Dashboard() {
  const { organization } = useAuth();

  const { data: stats, isLoading } = useQuery({
    queryKey: ['dashboard-stats'],
    queryFn: async () => {
      try {
        return await api.get<DashboardStats>('/dashboard/stats');
      } catch (err) {
        handleApiError(err);
      }
    },
  });

  const { data: activity } = useQuery({
    queryKey: ['dashboard-activity'],
    queryFn: async () => {
      try {
        return await api.get<ActivityEntry[]>('/dashboard/activity');
      } catch (err) {
        handleApiError(err);
        return [];
      }
    },
  });

  return (
    <div>
      {/* Page header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">Dashboard</h1>
          <p className="mt-1 text-sm text-slate-500">
            Welcome back{organization ? `, ${organization.name}` : ''}. Here's your organization overview.
          </p>
        </div>
        {organization && (
          <div className="flex items-center gap-2 px-3.5 py-2 bg-white rounded-lg border border-slate-200">
            <span className="text-sm font-medium text-slate-900">{organization.name}</span>
          </div>
        )}
      </div>

      {/* Stats cards */}
      {isLoading ? (
        <div className="grid grid-cols-1 md:grid-cols-3 gap-5">
          {[1, 2, 3].map((i) => (
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
        <div className="grid grid-cols-1 md:grid-cols-3 gap-5">
          <StatCard
            label="Active API Keys"
            value={stats?.active_api_keys ?? 0}
            icon={Key}
          />
          <StatCard
            label="API Calls (24h)"
            value={stats?.total_requests_today?.toLocaleString() ?? '0'}
            icon={BarChart3}
          />
          <StatCard
            label="Scopes Granted"
            value={stats?.scopes_granted ?? 0}
            icon={Shield}
          />
        </div>
      )}

      {/* Bottom section: Quick Start + Recent Activity */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-5 mt-8">
        {/* Quick Start */}
        <div className="bg-white rounded-xl border border-slate-200 p-6">
          <h2 className="text-base font-semibold text-slate-900 mb-4">Quick Start</h2>
          <div className="space-y-4">
            {[
              'Create your organization',
              'Generate an API key',
              'Make your first API call',
            ].map((step, i) => (
              <div key={i} className="flex items-center gap-3">
                <div className="w-7 h-7 rounded-full bg-cyan-400 text-[#0A0F1C] text-sm font-bold flex items-center justify-center shrink-0">
                  {i + 1}
                </div>
                <span className="text-sm text-slate-700">{step}</span>
              </div>
            ))}
          </div>
        </div>

        {/* Recent Activity */}
        <div className="bg-white rounded-xl border border-slate-200 p-6">
          <h2 className="text-base font-semibold text-slate-900 mb-4">Recent Activity</h2>
          <div className="space-y-3">
            {(!activity || activity.length === 0) ? (
              <p className="text-sm text-slate-500 text-center py-4">
                No recent activity to display
              </p>
            ) : (
              activity.map((entry) => (
                <div key={entry.id} className="flex items-start gap-3 py-2 border-b border-slate-100 last:border-0">
                  <Clock className="w-4 h-4 text-slate-400 mt-0.5 shrink-0" />
                  <div className="flex-1 min-w-0">
                    <p className="text-sm text-slate-900">{formatAction(entry.action)}</p>
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
              ))
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
