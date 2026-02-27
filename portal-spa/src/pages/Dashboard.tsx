import { useQuery } from '@tanstack/react-query';
import { Key, BarChart3, Shield } from 'lucide-react';
import { useAuth } from '../hooks/useAuth.ts';
import { api } from '../api/client.ts';
import { handleApiError } from '../hooks/useAuth.ts';
import type { DashboardStats } from '../types/index.ts';
import type { LucideIcon } from 'lucide-react';

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
            value={stats?.total_api_keys ?? 0}
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
          <div className="space-y-4">
            <p className="text-sm text-slate-500 text-center py-4">
              No recent activity to display
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
