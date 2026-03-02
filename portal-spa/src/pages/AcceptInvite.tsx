import { useState, useEffect, type FormEvent } from "react";
import { useSearchParams, useNavigate } from "react-router-dom";
import { Landmark } from "lucide-react";
import { api, ApiClientError } from "../api/client.ts";
import type { InviteInfo, AcceptInviteRequest } from "../types/index.ts";

export function AcceptInvite() {
  const [searchParams] = useSearchParams();
  const navigate = useNavigate();
  const token = searchParams.get("token") ?? "";

  const [inviteInfo, setInviteInfo] = useState<InviteInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [validationError, setValidationError] = useState("");

  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [confirmPassword, setConfirmPassword] = useState("");
  const [submitError, setSubmitError] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!token) {
      setValidationError("Missing invite token.");
      setLoading(false);
      return;
    }

    api
      .get<InviteInfo>(`/auth/invite?token=${encodeURIComponent(token)}`)
      .then((info) => {
        setInviteInfo(info);
        setLoading(false);
      })
      .catch((err) => {
        if (err instanceof ApiClientError) {
          setValidationError(err.apiError.message);
        } else {
          setValidationError("Failed to validate invitation.");
        }
        setLoading(false);
      });
  }, [token]);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setSubmitError("");

    if (password.length < 8) {
      setSubmitError("Password must be at least 8 characters.");
      return;
    }
    if (password !== confirmPassword) {
      setSubmitError("Passwords do not match.");
      return;
    }

    setSubmitting(true);
    try {
      await api.post<void>("/auth/invite/accept", {
        token,
        password,
        name: name || undefined
      } as AcceptInviteRequest);
      navigate("/", { replace: true });
    } catch (err) {
      if (err instanceof ApiClientError) {
        setSubmitError(err.apiError.message);
      } else {
        setSubmitError("An unexpected error occurred.");
      }
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[#0A0F1C]">
      <div className="w-full max-w-[420px] mx-4">
        <div className="bg-[#1E293B] rounded-xl p-10">
          {/* Logo */}
          <div className="flex flex-col items-center gap-2 mb-8">
            <Landmark className="w-10 h-10 text-cyan-400" />
            <span className="font-mono text-[28px] font-bold text-white">
              Bankie
            </span>
            <span className="text-sm text-slate-400">Accept Invitation</span>
          </div>

          {loading && (
            <div className="text-center">
              <p className="text-sm text-slate-400">Validating invitation...</p>
            </div>
          )}

          {validationError && (
            <div className="text-center">
              <div
                className="mb-4 p-3 bg-red-900/30 border border-red-800 text-red-400 rounded-md text-sm"
                role="alert"
              >
                {validationError}
              </div>
              <a
                href="/login"
                className="text-sm text-cyan-400 hover:text-cyan-300"
              >
                Go to Login
              </a>
            </div>
          )}

          {inviteInfo && (
            <>
              <div className="mb-6 p-4 bg-[#0F172A] rounded-lg border border-slate-600">
                <p className="text-sm text-slate-300">
                  You&apos;ve been invited to join{" "}
                  <span className="font-semibold text-white">
                    {inviteInfo.org_name}
                  </span>{" "}
                  as{" "}
                  <span className="font-semibold text-cyan-400 capitalize">
                    {inviteInfo.role}
                  </span>
                  .
                </p>
                <p className="mt-1 text-xs text-slate-500">
                  {inviteInfo.email}
                </p>
              </div>

              {submitError && (
                <div
                  className="mb-4 p-3 bg-red-900/30 border border-red-800 text-red-400 rounded-md text-sm"
                  role="alert"
                >
                  {submitError}
                </div>
              )}

              <form onSubmit={handleSubmit} className="space-y-4">
                <div>
                  <label
                    htmlFor="full-name"
                    className="block text-sm font-medium text-slate-300 mb-1.5"
                  >
                    Full name
                  </label>
                  <input
                    id="full-name"
                    type="text"
                    autoComplete="name"
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    className="w-full px-3.5 py-2.5 bg-[#0F172A] border border-slate-600 rounded-lg text-sm text-white
                      placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
                    placeholder="Jane Doe"
                  />
                </div>

                <div>
                  <label
                    htmlFor="password"
                    className="block text-sm font-medium text-slate-300 mb-1.5"
                  >
                    Create Password
                  </label>
                  <input
                    id="password"
                    type="password"
                    required
                    autoComplete="new-password"
                    minLength={8}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    className="w-full px-3.5 py-2.5 bg-[#0F172A] border border-slate-600 rounded-lg text-sm text-white
                      placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
                    placeholder="Minimum 8 characters"
                  />
                </div>

                <div>
                  <label
                    htmlFor="confirm-password"
                    className="block text-sm font-medium text-slate-300 mb-1.5"
                  >
                    Confirm Password
                  </label>
                  <input
                    id="confirm-password"
                    type="password"
                    required
                    autoComplete="new-password"
                    minLength={8}
                    value={confirmPassword}
                    onChange={(e) => setConfirmPassword(e.target.value)}
                    className="w-full px-3.5 py-2.5 bg-[#0F172A] border border-slate-600 rounded-lg text-sm text-white
                      placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
                    placeholder="Re-enter password"
                  />
                </div>

                <button
                  type="submit"
                  disabled={submitting}
                  className="w-full h-11 bg-cyan-400 text-[#0A0F1C] rounded-lg text-[15px] font-semibold
                    hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2
                    focus:ring-offset-[#1E293B] disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {submitting ? "Setting up..." : "Accept & Join"}
                </button>
              </form>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
