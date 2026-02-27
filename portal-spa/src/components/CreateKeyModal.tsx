import { useState, type FormEvent } from "react";
import type { CreateApiKeyRequest } from "../types/index.ts";

const AVAILABLE_SCOPES = [
  "accounts:read",
  "accounts:write",
  "ledgers:read",
  "ledgers:write",
  "transactions:read",
  "reports:read"
];

interface CreateKeyModalProps {
  onClose: () => void;
  onSubmit: (data: CreateApiKeyRequest) => void;
  isLoading: boolean;
  error: string | null;
}

export function CreateKeyModal({
  onClose,
  onSubmit,
  isLoading,
  error
}: CreateKeyModalProps) {
  const [name, setName] = useState("");
  const [scopes, setScopes] = useState<Set<string>>(new Set());

  function toggleScope(scope: string) {
    setScopes((prev) => {
      const next = new Set(prev);
      if (next.has(scope)) {
        next.delete(scope);
      } else {
        next.add(scope);
      }
      return next;
    });
  }

  function handleSubmit(e: FormEvent) {
    e.preventDefault();
    onSubmit({ name, scopes: Array.from(scopes) });
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-key-title"
    >
      <div className="bg-white rounded-xl shadow-xl w-full max-w-md mx-4">
        <div className="px-6 py-4 border-b border-slate-200">
          <h2
            id="create-key-title"
            className="text-lg font-semibold text-slate-900"
          >
            Create API Key
          </h2>
        </div>

        <form onSubmit={handleSubmit} className="px-6 py-4 space-y-4">
          {error && (
            <div
              className="p-3 bg-red-50 border border-red-200 text-red-700 rounded-md text-sm"
              role="alert"
            >
              {error}
            </div>
          )}

          <div>
            <label
              htmlFor="key-name"
              className="block text-sm font-medium text-slate-700 mb-1"
            >
              Name
            </label>
            <input
              id="key-name"
              type="text"
              required
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="w-full px-3 py-2 border border-slate-300 rounded-lg text-sm
                focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
              placeholder="e.g., Production Backend"
            />
          </div>

          <fieldset>
            <legend className="block text-sm font-medium text-slate-700 mb-2">
              Scopes
            </legend>
            <div className="space-y-2">
              {AVAILABLE_SCOPES.map((scope) => (
                <label
                  key={scope}
                  className="flex items-center gap-2 cursor-pointer"
                >
                  <input
                    type="checkbox"
                    checked={scopes.has(scope)}
                    onChange={() => toggleScope(scope)}
                    className="rounded text-cyan-400 focus:ring-cyan-400"
                  />
                  <span className="text-sm text-slate-700 font-mono">
                    {scope}
                  </span>
                </label>
              ))}
            </div>
            <p className="mt-1 text-xs text-slate-400">
              Leave unchecked for full access
            </p>
          </fieldset>
        </form>

        <div className="px-6 py-4 border-t border-slate-200 flex justify-end gap-3">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2 text-sm text-slate-700 hover:text-slate-900"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={isLoading || !name.trim()}
            className="px-4 py-2 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
              hover:bg-cyan-500 disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isLoading ? "Creating..." : "Create key"}
          </button>
        </div>
      </div>
    </div>
  );
}
