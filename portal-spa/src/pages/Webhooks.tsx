import { useState } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Plus,
  Trash2,
  ChevronDown,
  ChevronRight,
  Copy,
  Check,
  Radio
} from "lucide-react";
import { api } from "../api/client.ts";
import { useAuth, handleApiError } from "../hooks/useAuth.ts";
import { ConfirmModal } from "../components/ConfirmModal.tsx";
import { useToast } from "../hooks/useToast.tsx";
import type {
  WebhookEndpoint,
  WebhookDelivery,
  CreateWebhookEndpointResponse
} from "../types/index.ts";

const VALID_EVENT_TYPES = [
  "account.opened",
  "account.approved",
  "account.frozen",
  "account.closed",
  "transaction.completed",
  "transaction.failed"
];

function StatusBadge({ status }: { status: string }) {
  const colors: Record<string, string> = {
    active: "bg-green-50 text-green-700",
    disabled: "bg-red-50 text-red-700",
    pending: "bg-amber-50 text-amber-700",
    success: "bg-green-50 text-green-700",
    failed: "bg-red-50 text-red-700",
    dead_letter: "bg-slate-100 text-slate-600"
  };
  return (
    <span
      className={`px-2 py-0.5 rounded-full text-xs font-medium ${colors[status] ?? "bg-slate-100 text-slate-600"}`}
    >
      {status}
    </span>
  );
}

function DeliveryTable({ endpointId }: { endpointId: string }) {
  const [statusFilter, setStatusFilter] = useState<string>("");
  const [page, setPage] = useState(1);

  const { data } = useQuery({
    queryKey: ["webhook-deliveries", endpointId, statusFilter, page],
    queryFn: async () => {
      try {
        const params = new URLSearchParams({
          page: String(page),
          per_page: "10"
        });
        if (statusFilter) params.set("status", statusFilter);
        return await api.get<{ deliveries: WebhookDelivery[]; total: number }>(
          `/webhooks/${endpointId}/deliveries?${params}`
        );
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const deliveries = data?.deliveries ?? [];
  const total = data?.total ?? 0;
  const totalPages = Math.ceil(total / 10);

  return (
    <div className="pl-8 pr-4 pb-4">
      <div className="flex items-center gap-2 mb-3">
        <span className="text-xs font-medium text-slate-500">
          Filter status:
        </span>
        {["", "pending", "success", "failed", "dead_letter"].map((s) => (
          <button
            key={s}
            onClick={() => {
              setStatusFilter(s);
              setPage(1);
            }}
            className={`px-2 py-0.5 text-xs rounded ${statusFilter === s ? "bg-cyan-400 text-[#0A0F1C] font-semibold" : "bg-slate-100 text-slate-600 hover:bg-slate-200"}`}
          >
            {s || "All"}
          </button>
        ))}
      </div>

      {deliveries.length === 0 ? (
        <p className="text-sm text-slate-400">No deliveries found.</p>
      ) : (
        <>
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-slate-400 border-b border-slate-100">
                <th className="pb-2 font-medium">Event</th>
                <th className="pb-2 font-medium">Status</th>
                <th className="pb-2 font-medium">HTTP</th>
                <th className="pb-2 font-medium">Latency</th>
                <th className="pb-2 font-medium">Attempt</th>
                <th className="pb-2 font-medium">Created</th>
              </tr>
            </thead>
            <tbody>
              {deliveries.map((d) => (
                <tr
                  key={d.id}
                  className="border-b border-slate-50 hover:bg-slate-50/50"
                >
                  <td className="py-2 font-mono text-xs">{d.event_type}</td>
                  <td className="py-2">
                    <StatusBadge status={d.status} />
                  </td>
                  <td className="py-2 text-xs">{d.http_status ?? "—"}</td>
                  <td className="py-2 text-xs">
                    {d.latency_ms != null ? `${d.latency_ms}ms` : "—"}
                  </td>
                  <td className="py-2 text-xs">{d.attempt_number}</td>
                  <td className="py-2 text-xs text-slate-400">
                    {new Date(d.created_at).toLocaleString()}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {totalPages > 1 && (
            <div className="flex justify-center gap-2 mt-3">
              <button
                onClick={() => setPage((p) => Math.max(1, p - 1))}
                disabled={page === 1}
                className="px-2 py-1 text-xs rounded bg-slate-100 text-slate-600 hover:bg-slate-200 disabled:opacity-50"
              >
                Prev
              </button>
              <span className="px-2 py-1 text-xs text-slate-500">
                {page} / {totalPages}
              </span>
              <button
                onClick={() => setPage((p) => Math.min(totalPages, p + 1))}
                disabled={page === totalPages}
                className="px-2 py-1 text-xs rounded bg-slate-100 text-slate-600 hover:bg-slate-200 disabled:opacity-50"
              >
                Next
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

export function Webhooks() {
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();
  const canManage = user?.role === "owner" || user?.role === "admin";

  const [showCreate, setShowCreate] = useState(false);
  const [newSecret, setNewSecret] = useState<string | null>(null);
  const [copiedSecret, setCopiedSecret] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);

  // Form state
  const [formUrl, setFormUrl] = useState("");
  const [formDesc, setFormDesc] = useState("");
  const [formEvents, setFormEvents] = useState<string[]>([]);

  const { data: endpoints = [], isLoading } = useQuery({
    queryKey: ["webhook-endpoints"],
    queryFn: async () => {
      try {
        return await api.get<WebhookEndpoint[]>("/webhooks");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const createMutation = useMutation({
    mutationFn: async () => {
      return await api.post<CreateWebhookEndpointResponse>("/webhooks", {
        url: formUrl,
        event_types: formEvents,
        description: formDesc || undefined
      });
    },
    onSuccess: (data) => {
      queryClient.invalidateQueries({ queryKey: ["webhook-endpoints"] });
      setNewSecret(data.signing_secret);
      setShowCreate(false);
      setFormUrl("");
      setFormDesc("");
      setFormEvents([]);
      toast.success("Webhook endpoint created");
    },
    onError: (err) => toast.error(err.message)
  });

  const deleteMutation = useMutation({
    mutationFn: async (id: string) => {
      return await api.delete<void>(`/webhooks/${id}`);
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["webhook-endpoints"] });
      setConfirmDelete(null);
      toast.success("Webhook endpoint deleted");
    },
    onError: (err) => toast.error(err.message)
  });

  const toggleEvent = (evt: string) => {
    setFormEvents((prev) =>
      prev.includes(evt) ? prev.filter((e) => e !== evt) : [...prev, evt]
    );
  };

  const copySecret = async () => {
    if (newSecret) {
      await navigator.clipboard.writeText(newSecret);
      setCopiedSecret(true);
      setTimeout(() => setCopiedSecret(false), 2000);
    }
  };

  const isValidUrl = formUrl.startsWith("https://") && formUrl.length > 10;

  return (
    <div>
      <div className="flex items-center justify-between mb-8">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">Webhooks</h1>
          <p className="text-sm text-slate-500 mt-1">
            Manage webhook endpoints and monitor deliveries
          </p>
        </div>
        {canManage && (
          <button
            onClick={() => setShowCreate(true)}
            className="flex items-center gap-1.5 px-4 py-2 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg hover:bg-cyan-300 transition-colors"
          >
            <Plus className="w-4 h-4" />
            Add Endpoint
          </button>
        )}
      </div>

      {/* Secret reveal modal */}
      {newSecret && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="bg-white rounded-xl shadow-xl p-6 max-w-lg w-full mx-4">
            <h3 className="text-lg font-bold text-slate-900 mb-2">
              Signing Secret Created
            </h3>
            <p className="text-sm text-slate-500 mb-4">
              Copy this secret now. It will not be shown again.
            </p>
            <div className="flex items-center gap-2 bg-slate-50 rounded-lg px-3 py-2 mb-4">
              <code className="text-sm font-mono text-slate-800 flex-1 break-all">
                {newSecret}
              </code>
              <button
                onClick={copySecret}
                className="p-1.5 rounded hover:bg-slate-200 transition-colors"
              >
                {copiedSecret ? (
                  <Check className="w-4 h-4 text-green-600" />
                ) : (
                  <Copy className="w-4 h-4 text-slate-500" />
                )}
              </button>
            </div>
            <button
              onClick={() => setNewSecret(null)}
              className="w-full py-2 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg hover:bg-cyan-300 transition-colors"
            >
              Done
            </button>
          </div>
        </div>
      )}

      {/* Create endpoint modal */}
      {showCreate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
          <div className="bg-white rounded-xl shadow-xl p-6 max-w-lg w-full mx-4">
            <h3 className="text-lg font-bold text-slate-900 mb-4">
              New Webhook Endpoint
            </h3>

            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">
                  URL (HTTPS required)
                </label>
                <input
                  type="url"
                  value={formUrl}
                  onChange={(e) => setFormUrl(e.target.value)}
                  placeholder="https://example.com/webhook"
                  className="w-full px-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 mb-1">
                  Description (optional)
                </label>
                <input
                  type="text"
                  value={formDesc}
                  onChange={(e) => setFormDesc(e.target.value)}
                  placeholder="Production webhook"
                  className="w-full px-3 py-2 border border-slate-200 rounded-lg text-sm focus:outline-none focus:ring-2 focus:ring-cyan-400"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-slate-700 mb-2">
                  Event Types
                </label>
                <div className="grid grid-cols-2 gap-2">
                  {VALID_EVENT_TYPES.map((evt) => (
                    <label
                      key={evt}
                      className="flex items-center gap-2 text-sm cursor-pointer"
                    >
                      <input
                        type="checkbox"
                        checked={formEvents.includes(evt)}
                        onChange={() => toggleEvent(evt)}
                        className="rounded border-slate-300 text-cyan-400 focus:ring-cyan-400"
                      />
                      <span className="font-mono text-xs">{evt}</span>
                    </label>
                  ))}
                </div>
              </div>
            </div>

            {createMutation.error && (
              <p className="mt-3 text-sm text-red-600">
                {createMutation.error.message}
              </p>
            )}

            <div className="flex justify-end gap-3 mt-6">
              <button
                onClick={() => setShowCreate(false)}
                className="px-4 py-2 text-sm text-slate-600 hover:text-slate-900 transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={() => createMutation.mutate()}
                disabled={
                  !isValidUrl ||
                  formEvents.length === 0 ||
                  createMutation.isPending
                }
                className="px-4 py-2 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg hover:bg-cyan-300 transition-colors disabled:opacity-50"
              >
                {createMutation.isPending ? "Creating..." : "Create Endpoint"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Endpoints list */}
      {isLoading ? (
        <div className="space-y-3">
          {[1, 2, 3].map((i) => (
            <div
              key={i}
              className="h-20 bg-slate-100 rounded-xl animate-pulse"
            />
          ))}
        </div>
      ) : endpoints.length === 0 ? (
        <div className="text-center py-16">
          <Radio className="w-12 h-12 text-slate-300 mx-auto mb-4" />
          <h3 className="text-lg font-semibold text-slate-700 mb-1">
            No webhook endpoints
          </h3>
          <p className="text-sm text-slate-500">
            Create your first endpoint to start receiving events.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {endpoints.map((ep) => (
            <div
              key={ep.id}
              className="bg-white border border-slate-200 rounded-xl overflow-hidden"
            >
              <div className="flex items-center gap-4 px-5 py-4">
                <button
                  onClick={() =>
                    setExpandedId(expandedId === ep.id ? null : ep.id)
                  }
                  className="p-1 rounded hover:bg-slate-100 transition-colors"
                >
                  {expandedId === ep.id ? (
                    <ChevronDown className="w-4 h-4 text-slate-400" />
                  ) : (
                    <ChevronRight className="w-4 h-4 text-slate-400" />
                  )}
                </button>

                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-1">
                    <span className="font-mono text-sm text-slate-800 truncate">
                      {ep.url}
                    </span>
                    <StatusBadge status={ep.status} />
                  </div>
                  <div className="flex items-center gap-3 text-xs text-slate-400">
                    <span>
                      {ep.event_types.length} event
                      {ep.event_types.length !== 1 ? "s" : ""}
                    </span>
                    {ep.failure_count > 0 && (
                      <span className="text-amber-600">
                        {ep.failure_count} failure
                        {ep.failure_count !== 1 ? "s" : ""}
                      </span>
                    )}
                    <span>{new Date(ep.created_at).toLocaleDateString()}</span>
                    {ep.description && (
                      <span className="text-slate-400 truncate max-w-[200px]">
                        {ep.description}
                      </span>
                    )}
                  </div>
                </div>

                {canManage && (
                  <button
                    onClick={() => setConfirmDelete(ep.id)}
                    className="p-2 rounded hover:bg-red-50 text-slate-400 hover:text-red-600 transition-colors"
                    title="Delete endpoint"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                )}
              </div>

              {expandedId === ep.id && <DeliveryTable endpointId={ep.id} />}
            </div>
          ))}
        </div>
      )}

      {/* Delete confirmation */}
      {confirmDelete && (
        <ConfirmModal
          title="Delete Webhook Endpoint"
          message="This will permanently remove this endpoint and stop all webhook deliveries. This action cannot be undone."
          confirmLabel="Delete"
          destructive
          onConfirm={() => deleteMutation.mutate(confirmDelete)}
          onCancel={() => setConfirmDelete(null)}
          isLoading={deleteMutation.isPending}
        />
      )}
    </div>
  );
}
