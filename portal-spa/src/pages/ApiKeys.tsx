import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Eye, EyeOff, AlertTriangle } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError, useAuth } from "../hooks/useAuth.ts";
import type {
  ApiKey,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  RotateApiKeyResponse,
  RateLimitEntry
} from "../types/index.ts";
import { CreateKeyModal } from "../components/CreateKeyModal.tsx";
import { ConfirmModal } from "../components/ConfirmModal.tsx";
import { KeyRevealBanner } from "../components/KeyRevealBanner.tsx";
import { useToast } from "../hooks/useToast.tsx";

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    active: "bg-green-50 text-green-700",
    rotated: "bg-amber-50 text-amber-700",
    revoked: "bg-red-50 text-red-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium capitalize ${
        styles[status] ?? "bg-slate-100 text-slate-600"
      }`}
    >
      {status}
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

function formatGraceCountdown(expiresAt: string): string {
  const now = new Date();
  const expires = new Date(expiresAt);
  const diffMs = expires.getTime() - now.getTime();
  if (diffMs <= 0) return "Expired";
  const hours = Math.floor(diffMs / 3600000);
  const minutes = Math.floor((diffMs % 3600000) / 60000);
  if (hours > 0) return `${hours}h ${minutes}m remaining`;
  return `${minutes}m remaining`;
}

function RateLimitCell({ entry }: { entry: RateLimitEntry | undefined }) {
  if (!entry) {
    return <span className="text-xs text-slate-300">&mdash;</span>;
  }
  const requests = entry.requests_24h ?? 0;
  const throttled = entry.throttled_24h ?? 0;
  const total = requests + throttled;
  const isHealthy = throttled === 0;
  const isCritical = total > 0 && Math.round((throttled / total) * 100) >= 25;
  const barColor = isHealthy
    ? "bg-green-400"
    : isCritical
      ? "bg-red-500"
      : "bg-amber-400";
  const cap = entry.sustained_per_min > 0 ? entry.sustained_per_min : 1;
  const usagePct = Math.min(100, Math.round((requests / cap) * 100));

  return (
    <div className="w-36">
      <span className="text-[11px] text-slate-500">
        <span className="font-semibold text-slate-700">
          {requests.toLocaleString()}
        </span>
        {" / "}
        {entry.sustained_per_min.toLocaleString()} req/min
      </span>
      <div className="w-full h-1.5 bg-slate-200 rounded-full overflow-hidden mt-1">
        <div
          className={`h-full rounded-full transition-all ${barColor}`}
          style={{ width: `${usagePct}%` }}
        />
      </div>
    </div>
  );
}

export function ApiKeys() {
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();
  const canManageKeys = user?.role === "owner" || user?.role === "admin";

  const [showCreateModal, setShowCreateModal] = useState(false);
  const [rawKey, setRawKey] = useState<string | null>(null);
  const [confirmAction, setConfirmAction] = useState<{
    type: "rotate" | "revoke";
    keyId: string;
    keyName: string;
  } | null>(null);
  const [expandedKey, setExpandedKey] = useState<string | null>(null);

  const { data: keys = [], isLoading } = useQuery({
    queryKey: ["api-keys"],
    queryFn: async () => {
      try {
        return await api.get<ApiKey[]>("/api-keys");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const { data: rateLimits = [] } = useQuery({
    queryKey: ["dashboard-rate-limits"],
    queryFn: async () => {
      try {
        return await api.get<RateLimitEntry[]>("/dashboard/rate-limits");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const rateLimitMap = new Map(rateLimits.map((rl) => [rl.key_id, rl]));

  const createMutation = useMutation({
    mutationFn: (data: CreateApiKeyRequest) =>
      api.post<CreateApiKeyResponse>("/api-keys", data),
    onSuccess: (response) => {
      setRawKey(response.raw_key);
      setShowCreateModal(false);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
      toast.success("API key created successfully");
    },
    onError: (err) => toast.error(err.message)
  });

  const rotateMutation = useMutation({
    mutationFn: (keyId: string) =>
      api.post<RotateApiKeyResponse>(`/api-keys/${keyId}/rotate`, {}),
    onSuccess: (response) => {
      setRawKey(response.new_key.raw_key);
      setConfirmAction(null);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
      toast.success("API key rotated — grace period active");
    },
    onError: (err) => toast.error(err.message)
  });

  const revokeMutation = useMutation({
    mutationFn: (keyId: string) => api.delete<void>(`/api-keys/${keyId}`),
    onSuccess: () => {
      setConfirmAction(null);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
      toast.success("API key revoked");
    },
    onError: (err) => toast.error(err.message)
  });

  function handleConfirm() {
    if (!confirmAction) return;
    if (confirmAction.type === "rotate") {
      rotateMutation.mutate(confirmAction.keyId);
    } else {
      revokeMutation.mutate(confirmAction.keyId);
    }
  }

  // Check if any keys have active grace periods
  const graceKeys = keys.filter(
    (k) => k.status === "rotated" && k.grace_expires_at
  );

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">API Keys</h1>
          <p className="mt-1 text-sm text-slate-500">
            Manage your API keys for programmatic access.
          </p>
        </div>
        {canManageKeys && (
          <button
            onClick={() => setShowCreateModal(true)}
            className="flex items-center gap-2 h-10 px-5 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
              hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2"
            aria-label="Create new API key"
          >
            <Plus className="w-4 h-4" />
            Create Key
          </button>
        )}
      </div>

      {rawKey && (
        <KeyRevealBanner rawKey={rawKey} onDismiss={() => setRawKey(null)} />
      )}

      {/* Grace period banner */}
      {graceKeys.length > 0 && (
        <div className="mb-4 p-4 bg-amber-50 border border-amber-200 rounded-xl flex items-start gap-3">
          <AlertTriangle className="w-5 h-5 text-amber-500 shrink-0 mt-0.5" />
          <div>
            <p className="text-sm font-medium text-amber-800">
              Grace period active for {graceKeys.length} rotated key
              {graceKeys.length > 1 ? "s" : ""}
            </p>
            {graceKeys.map((k) => (
              <p key={k.id} className="text-xs text-amber-600 mt-1">
                <span className="font-mono">{k.name}</span> &mdash;{" "}
                {k.grace_expires_at
                  ? formatGraceCountdown(k.grace_expires_at)
                  : "N/A"}
              </p>
            ))}
          </div>
        </div>
      )}

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
      ) : keys.length === 0 ? (
        <div className="bg-white rounded-xl border border-slate-200 p-12 text-center">
          <p className="text-slate-500 mb-4">No API keys yet</p>
          {canManageKeys && (
            <button
              onClick={() => setShowCreateModal(true)}
              className="text-cyan-500 text-sm font-medium hover:text-cyan-600"
            >
              Create your first API key
            </button>
          )}
        </div>
      ) : (
        <div className="bg-white rounded-xl border border-slate-200 overflow-hidden">
          <table className="w-full" aria-label="API keys">
            <thead>
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Name
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Key Prefix
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[120px]">
                  Status
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[140px]">
                  Usage (24h)
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Created
                </th>
                {canManageKeys && (
                  <th className="text-right px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[140px]">
                    Actions
                  </th>
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {keys.map((key) => (
                <KeyRow
                  key={key.id}
                  apiKey={key}
                  rateLimitEntry={rateLimitMap.get(key.id)}
                  canManage={canManageKeys}
                  isExpanded={expandedKey === key.id}
                  onToggleExpand={() =>
                    setExpandedKey(expandedKey === key.id ? null : key.id)
                  }
                  onRotate={() =>
                    setConfirmAction({
                      type: "rotate",
                      keyId: key.id,
                      keyName: key.name
                    })
                  }
                  onRevoke={() =>
                    setConfirmAction({
                      type: "revoke",
                      keyId: key.id,
                      keyName: key.name
                    })
                  }
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {showCreateModal && (
        <CreateKeyModal
          onClose={() => setShowCreateModal(false)}
          onSubmit={(data) => createMutation.mutate(data)}
          isLoading={createMutation.isPending}
          error={createMutation.error?.message ?? null}
        />
      )}

      {confirmAction && (
        <ConfirmModal
          title={`${confirmAction.type === "rotate" ? "Rotate" : "Revoke"} API Key`}
          message={
            confirmAction.type === "rotate"
              ? `This will invalidate the current key "${confirmAction.keyName}" and generate a new one. Any applications using the old key will stop working.`
              : `This will permanently revoke the key "${confirmAction.keyName}". This action cannot be undone.`
          }
          confirmLabel={
            confirmAction.type === "rotate" ? "Rotate key" : "Revoke key"
          }
          destructive={confirmAction.type === "revoke"}
          onConfirm={handleConfirm}
          onCancel={() => setConfirmAction(null)}
          isLoading={rotateMutation.isPending || revokeMutation.isPending}
        />
      )}
    </div>
  );
}

function KeyRow({
  apiKey,
  rateLimitEntry,
  canManage,
  isExpanded,
  onToggleExpand,
  onRotate,
  onRevoke
}: {
  apiKey: ApiKey;
  rateLimitEntry: RateLimitEntry | undefined;
  canManage: boolean;
  isExpanded: boolean;
  onToggleExpand: () => void;
  onRotate: () => void;
  onRevoke: () => void;
}) {
  const [showPrefix, setShowPrefix] = useState(false);
  const isRevoked = apiKey.status === "revoked";
  const textColor = isRevoked ? "text-slate-400" : "text-slate-900";
  const mutedColor = isRevoked ? "text-slate-300" : "text-slate-500";
  const colSpan = canManage ? 6 : 5;

  return (
    <>
      <tr
        className={`hover:bg-slate-50 cursor-pointer h-14 ${isRevoked ? "opacity-60" : ""}`}
        onClick={onToggleExpand}
        aria-expanded={isExpanded}
      >
        <td className={`px-6 py-3 text-sm font-medium ${textColor}`}>
          {apiKey.name}
        </td>
        <td className={`px-6 py-3 text-sm font-mono text-xs ${mutedColor}`}>
          <span className="inline-flex items-center gap-1.5">
            <span>
              {showPrefix ? `${apiKey.key_prefix}...` : "••••••••••••••••"}
            </span>
            <button
              onClick={(e) => {
                e.stopPropagation();
                setShowPrefix(!showPrefix);
              }}
              className="p-0.5 text-slate-400 hover:text-cyan-500 transition-colors"
              aria-label={showPrefix ? "Hide key prefix" : "Reveal key prefix"}
            >
              {showPrefix ? (
                <EyeOff className="w-3.5 h-3.5" />
              ) : (
                <Eye className="w-3.5 h-3.5" />
              )}
            </button>
          </span>
        </td>
        <td className="px-6 py-3">
          <StatusBadge status={apiKey.status} />
        </td>
        <td className="px-6 py-3">
          {isRevoked ? (
            <span className="text-xs text-slate-300">&mdash;</span>
          ) : (
            <RateLimitCell entry={rateLimitEntry} />
          )}
        </td>
        <td className={`px-6 py-3 text-sm ${mutedColor}`}>
          {formatDate(apiKey.created_at)}
        </td>
        {canManage && (
          <td className="px-6 py-3 text-right">
            {apiKey.status === "active" ? (
              <div className="flex items-center justify-end gap-2">
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onRotate();
                  }}
                  className="px-3 py-1.5 text-xs font-medium text-slate-700 bg-slate-100 rounded-md
                    hover:bg-slate-200 transition-colors"
                  aria-label={`Rotate key ${apiKey.name}`}
                >
                  Rotate
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    onRevoke();
                  }}
                  className="px-3 py-1.5 text-xs font-medium text-red-600 bg-red-50 rounded-md
                    hover:bg-red-100 transition-colors"
                  aria-label={`Revoke key ${apiKey.name}`}
                >
                  Revoke
                </button>
              </div>
            ) : (
              <span className="text-sm text-slate-300">&mdash;</span>
            )}
          </td>
        )}
      </tr>
      {isExpanded && (
        <tr>
          <td
            colSpan={colSpan}
            className="px-6 py-4 bg-slate-50 border-t border-slate-100"
          >
            <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-4 text-sm">
              <div>
                <p className="text-slate-500">Scopes</p>
                <p className="font-medium text-slate-900 mt-1">
                  {apiKey.scopes.length > 0 ? apiKey.scopes.join(", ") : "All"}
                </p>
              </div>
              <div>
                <p className="text-slate-500">Grace Expires</p>
                <p className="font-medium text-slate-900 mt-1">
                  {apiKey.grace_expires_at
                    ? formatDate(apiKey.grace_expires_at)
                    : "N/A"}
                </p>
              </div>
              <div>
                <p className="text-slate-500">Key ID</p>
                <p className="font-mono text-slate-900 mt-1 text-xs break-all">
                  {apiKey.id}
                </p>
              </div>
              {rateLimitEntry && (
                <>
                  <div>
                    <p className="text-slate-500">Requests (24h)</p>
                    <p className="font-medium text-slate-900 mt-1">
                      {(rateLimitEntry.requests_24h ?? 0).toLocaleString()}
                    </p>
                  </div>
                  <div>
                    <p className="text-slate-500">Throttled (24h)</p>
                    <p
                      className={`font-medium mt-1 ${rateLimitEntry.throttled_24h > 0 ? "text-amber-600" : "text-slate-900"}`}
                    >
                      {rateLimitEntry.throttled_24h.toLocaleString()}
                    </p>
                  </div>
                  <div>
                    <p className="text-slate-500">Rate Limit</p>
                    <p className="font-medium text-slate-900 mt-1">
                      {rateLimitEntry.sustained_per_min.toLocaleString()}{" "}
                      req/min
                    </p>
                  </div>
                </>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
