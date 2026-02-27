import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Plus, Eye, EyeOff } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError } from "../hooks/useAuth.ts";
import type {
  ApiKey,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  RotateApiKeyResponse
} from "../types/index.ts";
import { CreateKeyModal } from "../components/CreateKeyModal.tsx";
import { ConfirmModal } from "../components/ConfirmModal.tsx";
import { KeyRevealBanner } from "../components/KeyRevealBanner.tsx";

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

export function ApiKeys() {
  const queryClient = useQueryClient();
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

  const createMutation = useMutation({
    mutationFn: (data: CreateApiKeyRequest) =>
      api.post<CreateApiKeyResponse>("/api-keys", data),
    onSuccess: (response) => {
      setRawKey(response.raw_key);
      setShowCreateModal(false);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
    }
  });

  const rotateMutation = useMutation({
    mutationFn: (keyId: string) =>
      api.post<RotateApiKeyResponse>(`/api-keys/${keyId}/rotate`, {}),
    onSuccess: (response) => {
      setRawKey(response.new_key.raw_key);
      setConfirmAction(null);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
    }
  });

  const revokeMutation = useMutation({
    mutationFn: (keyId: string) => api.delete<void>(`/api-keys/${keyId}`),
    onSuccess: () => {
      setConfirmAction(null);
      queryClient.invalidateQueries({ queryKey: ["api-keys"] });
    }
  });

  function handleConfirm() {
    if (!confirmAction) return;
    if (confirmAction.type === "rotate") {
      rotateMutation.mutate(confirmAction.keyId);
    } else {
      revokeMutation.mutate(confirmAction.keyId);
    }
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">API Keys</h1>
          <p className="mt-1 text-sm text-slate-500">
            Manage your API keys for programmatic access.
          </p>
        </div>
        <button
          onClick={() => setShowCreateModal(true)}
          className="flex items-center gap-2 h-10 px-5 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
            hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2"
          aria-label="Create new API key"
        >
          <Plus className="w-4 h-4" />
          Create Key
        </button>
      </div>

      {rawKey && (
        <KeyRevealBanner rawKey={rawKey} onDismiss={() => setRawKey(null)} />
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
          <button
            onClick={() => setShowCreateModal(true)}
            className="text-cyan-500 text-sm font-medium hover:text-cyan-600"
          >
            Create your first API key
          </button>
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
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Created
                </th>
                <th className="text-right px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono w-[140px]">
                  Actions
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {keys.map((key) => (
                <KeyRow
                  key={key.id}
                  apiKey={key}
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
  isExpanded,
  onToggleExpand,
  onRotate,
  onRevoke
}: {
  apiKey: ApiKey;
  isExpanded: boolean;
  onToggleExpand: () => void;
  onRotate: () => void;
  onRevoke: () => void;
}) {
  const [showPrefix, setShowPrefix] = useState(false);
  const isRevoked = apiKey.status !== "active";
  const textColor = isRevoked ? "text-slate-400" : "text-slate-900";
  const mutedColor = isRevoked ? "text-slate-300" : "text-slate-500";

  return (
    <>
      <tr
        className="hover:bg-slate-50 cursor-pointer h-14"
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
        <td className={`px-6 py-3 text-sm ${mutedColor}`}>
          {formatDate(apiKey.created_at)}
        </td>
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
      </tr>
      {isExpanded && (
        <tr>
          <td
            colSpan={5}
            className="px-6 py-4 bg-slate-50 border-t border-slate-100"
          >
            <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
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
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
