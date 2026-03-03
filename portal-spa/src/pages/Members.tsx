import { useState, type FormEvent } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { UserPlus, Mail, Copy, Check } from "lucide-react";
import { api } from "../api/client.ts";
import { handleApiError, useAuth } from "../hooks/useAuth.ts";
import { ConfirmModal } from "../components/ConfirmModal.tsx";
import { useToast } from "../hooks/useToast.tsx";
import type {
  OrgMember,
  OrgRole,
  MemberStatus,
  InviteMemberRequest,
  InviteMemberResponse,
  UpdateRoleRequest
} from "../types/index.ts";

const AVATAR_COLORS = [
  "bg-cyan-500",
  "bg-violet-500",
  "bg-amber-500",
  "bg-rose-500",
  "bg-emerald-500",
  "bg-blue-500",
  "bg-pink-500",
  "bg-teal-500"
];

function avatarColor(id: string): string {
  let hash = 0;
  for (let i = 0; i < id.length; i++) {
    hash = (hash * 31 + id.charCodeAt(i)) | 0;
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function avatarInitial(name: string, email: string): string {
  if (name) return name.charAt(0).toUpperCase();
  return email.charAt(0).toUpperCase();
}

function displayName(member: OrgMember): string {
  return member.name || member.email.split("@")[0];
}

function RoleBadge({ role }: { role: OrgRole }) {
  const styles: Record<OrgRole, string> = {
    owner: "bg-purple-50 text-purple-700",
    admin: "bg-blue-50 text-blue-700",
    member: "bg-slate-100 text-slate-600"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium capitalize ${styles[role]}`}
    >
      {role}
    </span>
  );
}

function StatusBadge({ status }: { status: MemberStatus }) {
  const styles: Record<MemberStatus, string> = {
    active: "bg-green-50 text-green-700",
    pending: "bg-amber-50 text-amber-700",
    suspended: "bg-red-50 text-red-700"
  };
  return (
    <span
      className={`inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium capitalize ${styles[status]}`}
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

function capitalizeRole(role: string): string {
  return role.charAt(0).toUpperCase() + role.slice(1);
}

export function Members() {
  const { user } = useAuth();
  const queryClient = useQueryClient();
  const toast = useToast();
  const canManage = user?.role === "owner" || user?.role === "admin";

  const [showInviteForm, setShowInviteForm] = useState(false);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteRole, setInviteRole] = useState("member");
  const [changeRoleTarget, setChangeRoleTarget] = useState<OrgMember | null>(
    null
  );
  const [selectedRole, setSelectedRole] = useState<string>("member");
  const [removeTarget, setRemoveTarget] = useState<OrgMember | null>(null);
  const [resendTarget, setResendTarget] = useState<OrgMember | null>(null);
  const [inviteLink, setInviteLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const { data: members = [], isLoading } = useQuery({
    queryKey: ["members"],
    queryFn: async () => {
      try {
        return await api.get<OrgMember[]>("/members");
      } catch (err) {
        handleApiError(err);
      }
    }
  });

  const inviteMutation = useMutation({
    mutationFn: (data: InviteMemberRequest) =>
      api.post<InviteMemberResponse>("/members/invite", data),
    onSuccess: (response) => {
      setInviteEmail("");
      setInviteRole("member");
      setShowInviteForm(false);
      setInviteLink(response.invite_link);
      setCopied(false);
      queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Invitation sent successfully");
    },
    onError: (err) => toast.error(err.message)
  });

  const changeRoleMutation = useMutation({
    mutationFn: ({ id, role }: { id: string; role: string }) =>
      api.post<OrgMember>(`/members/${id}/role`, { role } as UpdateRoleRequest),
    onSuccess: () => {
      setChangeRoleTarget(null);
      queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Member role updated");
    },
    onError: (err) => toast.error(err.message)
  });

  const removeMutation = useMutation({
    mutationFn: (id: string) => api.delete<void>(`/members/${id}`),
    onSuccess: () => {
      setRemoveTarget(null);
      queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success("Member removed");
    },
    onError: (err) => toast.error(err.message)
  });

  const resendMutation = useMutation({
    mutationFn: (id: string) =>
      api.post<InviteMemberResponse>(`/members/${id}/resend-invite`, {}),
    onSuccess: (response) => {
      setResendTarget(null);
      setInviteLink(response.invite_link);
      setCopied(false);
      toast.success("Invitation resent");
    },
    onError: (err) => toast.error(err.message)
  });

  function handleInviteSubmit(e: FormEvent) {
    e.preventDefault();
    inviteMutation.mutate({ email: inviteEmail, role: inviteRole });
  }

  async function handleCopyLink() {
    if (!inviteLink) return;
    await navigator.clipboard.writeText(inviteLink);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  function openChangeRole(member: OrgMember) {
    setSelectedRole(member.role);
    setChangeRoleTarget(member);
  }

  return (
    <div>
      {/* Page header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-slate-900">Team Members</h1>
          <p className="mt-1 text-sm text-slate-500">
            Manage your organization's team members and permissions.
          </p>
        </div>
        {canManage && (
          <button
            onClick={() => setShowInviteForm(!showInviteForm)}
            className="flex items-center gap-2 h-10 px-5 bg-cyan-400 text-[#0A0F1C] text-sm font-semibold rounded-lg
              hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2"
            aria-label="Invite a new member"
          >
            <UserPlus className="w-4 h-4" />
            Invite Member
          </button>
        )}
      </div>

      {/* Members table */}
      {isLoading ? (
        <div className="bg-white rounded-xl border border-slate-200">
          {[1, 2, 3].map((i) => (
            <div
              key={i}
              className="p-4 border-b border-slate-100 animate-pulse"
            >
              <div className="flex items-center gap-4">
                <div className="h-9 w-9 bg-slate-200 rounded-full" />
                <div className="h-4 bg-slate-200 rounded w-32" />
                <div className="h-4 bg-slate-200 rounded w-48" />
                <div className="h-4 bg-slate-200 rounded w-16" />
                <div className="h-4 bg-slate-200 rounded w-24" />
                <div className="h-4 bg-slate-200 rounded w-16" />
              </div>
            </div>
          ))}
        </div>
      ) : members.length === 0 ? (
        <div className="bg-white rounded-xl border border-slate-200 p-12 text-center">
          <p className="text-slate-500">No team members found.</p>
        </div>
      ) : (
        <div className="bg-white rounded-xl border border-slate-200">
          <table className="w-full" aria-label="Team members">
            <thead>
              <tr className="border-b border-slate-200 bg-[#F8FAFC]">
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Member
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Email
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Role
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Joined
                </th>
                <th className="text-left px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                  Status
                </th>
                {canManage && (
                  <th className="text-right px-6 py-3 text-[11px] font-semibold text-slate-500 uppercase tracking-wider font-mono">
                    Actions
                  </th>
                )}
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-200">
              {members.map((member) => (
                <MemberRow
                  key={member.id}
                  member={member}
                  canManage={canManage}
                  isCurrentUser={member.id === user?.id}
                  currentUserRole={user?.role ?? "member"}
                  onChangeRole={() => openChangeRole(member)}
                  onRemove={() => setRemoveTarget(member)}
                  onResendInvite={() => setResendTarget(member)}
                />
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Invite form card */}
      {showInviteForm && canManage && (
        <div className="bg-white rounded-xl border border-slate-200 p-7 mt-6">
          <h2 className="text-base font-semibold text-slate-900 mb-4">
            Invite New Member
          </h2>
          <form onSubmit={handleInviteSubmit} className="flex items-end gap-4">
            <div className="flex-1">
              <label
                htmlFor="invite-email"
                className="block text-sm font-medium text-slate-500 mb-1.5"
              >
                Email Address
              </label>
              <input
                id="invite-email"
                type="email"
                value={inviteEmail}
                onChange={(e) => setInviteEmail(e.target.value)}
                placeholder="colleague@company.com"
                required
                className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-900
                  focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
              />
            </div>
            <div className="w-40">
              <label
                htmlFor="invite-role"
                className="block text-sm font-medium text-slate-500 mb-1.5"
              >
                Role
              </label>
              <select
                id="invite-role"
                value={inviteRole}
                onChange={(e) => setInviteRole(e.target.value)}
                className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-900
                  focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
              >
                <option value="member">Member</option>
                <option value="admin">Admin</option>
              </select>
            </div>
            <button
              type="submit"
              disabled={inviteMutation.isPending || !inviteEmail.trim()}
              className="h-11 px-5 text-sm font-semibold text-[#0A0F1C] bg-cyan-400 rounded-lg
                hover:bg-cyan-500 disabled:opacity-50 disabled:cursor-not-allowed transition-colors whitespace-nowrap"
            >
              {inviteMutation.isPending ? "Sending..." : "Send Invite"}
            </button>
          </form>
          {inviteMutation.error && (
            <p className="mt-3 text-sm text-red-600">
              {inviteMutation.error.message}
            </p>
          )}
        </div>
      )}

      {/* Change Role Modal */}
      {changeRoleTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          role="dialog"
          aria-modal="true"
          aria-labelledby="change-role-title"
        >
          <div className="bg-white rounded-xl shadow-xl w-full max-w-sm mx-4">
            <div className="px-6 py-5">
              <h2
                id="change-role-title"
                className="text-lg font-semibold text-slate-900"
              >
                Change Role
              </h2>

              {/* Member info header */}
              <div className="flex items-center gap-3 mt-4 mb-5">
                <div
                  className={`w-10 h-10 rounded-full flex items-center justify-center text-sm font-semibold text-white shrink-0 ${avatarColor(changeRoleTarget.id)}`}
                >
                  {avatarInitial(changeRoleTarget.name, changeRoleTarget.email)}
                </div>
                <div className="min-w-0">
                  <p className="text-sm font-semibold text-slate-900 truncate">
                    {displayName(changeRoleTarget)}
                  </p>
                  <p className="text-xs text-slate-500 truncate">
                    {changeRoleTarget.email}
                  </p>
                </div>
                <RoleBadge role={changeRoleTarget.role} />
              </div>

              <p className="text-xs font-medium text-slate-500 uppercase tracking-wider mb-3">
                Select new role
              </p>

              <div className="space-y-3">
                <label className="flex items-start gap-3 p-3 rounded-lg border border-slate-200 cursor-pointer hover:bg-slate-50">
                  <input
                    type="radio"
                    name="role"
                    value="admin"
                    checked={selectedRole === "admin"}
                    onChange={() => setSelectedRole("admin")}
                    className="mt-0.5"
                  />
                  <div>
                    <p className="text-sm font-medium text-slate-900">Admin</p>
                    <p className="text-xs text-slate-500">
                      Can manage API keys, invite members, and update org
                      settings.
                    </p>
                  </div>
                </label>
                <label className="flex items-start gap-3 p-3 rounded-lg border border-slate-200 cursor-pointer hover:bg-slate-50">
                  <input
                    type="radio"
                    name="role"
                    value="member"
                    checked={selectedRole === "member"}
                    onChange={() => setSelectedRole("member")}
                    className="mt-0.5"
                  />
                  <div>
                    <p className="text-sm font-medium text-slate-900">Member</p>
                    <p className="text-xs text-slate-500">
                      Read-only access. Cannot manage keys, members, or
                      settings.
                    </p>
                  </div>
                </label>
              </div>
            </div>

            <div className="px-6 py-4 border-t border-slate-200 flex justify-end gap-3">
              <button
                type="button"
                onClick={() => setChangeRoleTarget(null)}
                disabled={changeRoleMutation.isPending}
                className="px-4 py-2 text-sm text-slate-700 hover:text-slate-900"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() =>
                  changeRoleMutation.mutate({
                    id: changeRoleTarget.id,
                    role: selectedRole
                  })
                }
                disabled={
                  changeRoleMutation.isPending ||
                  selectedRole === changeRoleTarget.role
                }
                className="px-4 py-2 text-sm font-semibold rounded-lg bg-cyan-400 text-[#0A0F1C]
                  hover:bg-cyan-500 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {changeRoleMutation.isPending
                  ? "Saving..."
                  : `Change to ${capitalizeRole(selectedRole)}`}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Remove Member / Cancel Invite Confirmation */}
      {removeTarget && (
        <ConfirmModal
          title={
            removeTarget.status === "pending"
              ? "Cancel Invitation"
              : "Remove Member"
          }
          message={
            removeTarget.status === "pending"
              ? `Cancel the pending invitation for ${removeTarget.email}? The invite link will no longer work.`
              : `Are you sure you want to remove ${displayName(removeTarget)} (${removeTarget.email}) from the organization? They will lose all access.`
          }
          confirmLabel={
            removeTarget.status === "pending" ? "Cancel Invite" : "Remove"
          }
          destructive
          onConfirm={() => removeMutation.mutate(removeTarget.id)}
          onCancel={() => setRemoveTarget(null)}
          isLoading={removeMutation.isPending}
        />
      )}

      {/* Resend Invite Modal */}
      {resendTarget && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          role="dialog"
          aria-modal="true"
          aria-labelledby="resend-invite-title"
        >
          <div className="bg-white rounded-xl shadow-xl w-full max-w-sm mx-4">
            <div className="px-6 py-5 text-center">
              <div className="mx-auto w-12 h-12 rounded-full bg-cyan-50 flex items-center justify-center mb-4">
                <Mail className="w-6 h-6 text-cyan-600" />
              </div>
              <h2
                id="resend-invite-title"
                className="text-lg font-semibold text-slate-900"
              >
                Resend Invitation
              </h2>
              <p className="mt-2 text-sm text-slate-500">
                A new invitation email will be sent to:
              </p>
              <div className="mt-3 inline-flex items-center px-3 py-1.5 rounded-md bg-slate-800 text-white text-sm font-mono">
                {resendTarget.email}
              </div>
              <p className="mt-4 text-xs text-slate-400">
                The previous invitation link will be invalidated. The new link
                expires in 7 days.
              </p>
            </div>

            <div className="px-6 py-4 border-t border-slate-200 flex justify-end gap-3">
              <button
                type="button"
                onClick={() => setResendTarget(null)}
                disabled={resendMutation.isPending}
                className="px-4 py-2 text-sm text-slate-700 hover:text-slate-900"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => resendMutation.mutate(resendTarget.id)}
                disabled={resendMutation.isPending}
                className="px-4 py-2 text-sm font-semibold rounded-lg bg-cyan-400 text-[#0A0F1C]
                  hover:bg-cyan-500 disabled:opacity-50 disabled:cursor-not-allowed"
              >
                {resendMutation.isPending ? "Sending..." : "Resend Invite"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Invite Link Modal */}
      {inviteLink && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
          role="dialog"
          aria-modal="true"
          aria-labelledby="invite-link-title"
        >
          <div className="bg-white rounded-xl shadow-xl w-full max-w-lg mx-4">
            <div className="px-6 py-4">
              <h2
                id="invite-link-title"
                className="text-lg font-semibold text-slate-900"
              >
                Invitation Link
              </h2>
              <p className="mt-1 text-sm text-slate-500">
                Share this link with the invited member. It expires in 7 days.
              </p>

              <div className="mt-4 flex items-center gap-2">
                <input
                  type="text"
                  readOnly
                  value={inviteLink}
                  className="flex-1 h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-900 font-mono
                    focus:outline-none select-all"
                  onClick={(e) => (e.target as HTMLInputElement).select()}
                />
                <button
                  type="button"
                  onClick={handleCopyLink}
                  className="h-11 px-4 flex items-center gap-2 text-sm font-semibold rounded-lg border border-slate-200
                    hover:bg-slate-50 transition-colors shrink-0"
                >
                  {copied ? (
                    <>
                      <Check className="w-4 h-4 text-green-600" />
                      <span className="text-green-600">Copied</span>
                    </>
                  ) : (
                    <>
                      <Copy className="w-4 h-4 text-slate-600" />
                      <span className="text-slate-700">Copy</span>
                    </>
                  )}
                </button>
              </div>
            </div>

            <div className="px-6 py-4 border-t border-slate-200 flex justify-end">
              <button
                type="button"
                onClick={() => setInviteLink(null)}
                className="px-4 py-2 text-sm font-semibold rounded-lg bg-cyan-400 text-[#0A0F1C]
                  hover:bg-cyan-500"
              >
                Done
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function MemberRow({
  member,
  canManage,
  isCurrentUser,
  currentUserRole,
  onChangeRole,
  onRemove,
  onResendInvite
}: {
  member: OrgMember;
  canManage: boolean;
  isCurrentUser: boolean;
  currentUserRole: OrgRole;
  onChangeRole: () => void;
  onRemove: () => void;
  onResendInvite: () => void;
}) {
  const isOwner = member.role === "owner";
  const canShowActions = canManage && !isCurrentUser && !isOwner;
  const canChangeRole =
    currentUserRole === "owner" ||
    (currentUserRole === "admin" && member.role === "member");

  return (
    <tr className="hover:bg-slate-50 h-14">
      {/* MEMBER: avatar + name */}
      <td className="px-6 py-3">
        <div className="flex items-center gap-3">
          <div
            className={`w-9 h-9 rounded-full flex items-center justify-center text-sm font-semibold text-white shrink-0 ${avatarColor(member.id)}`}
          >
            {avatarInitial(member.name, member.email)}
          </div>
          <span className="text-sm font-semibold text-slate-900">
            {displayName(member)}
            {isCurrentUser && (
              <span className="ml-2 text-xs font-normal text-slate-400">
                (you)
              </span>
            )}
          </span>
        </div>
      </td>
      {/* EMAIL */}
      <td className="px-6 py-3 text-sm text-slate-500">{member.email}</td>
      {/* ROLE */}
      <td className="px-6 py-3">
        <RoleBadge role={member.role} />
      </td>
      {/* JOINED */}
      <td className="px-6 py-3 text-sm text-slate-500">
        {formatDate(member.created_at)}
      </td>
      {/* STATUS */}
      <td className="px-6 py-3">
        <StatusBadge status={member.status} />
      </td>
      {/* ACTIONS: inline text buttons */}
      {canManage && (
        <td className="px-6 py-3 text-right">
          {canShowActions ? (
            <div className="flex items-center justify-end gap-3">
              {canChangeRole && (
                <button
                  onClick={onChangeRole}
                  className="text-xs font-medium text-cyan-600 hover:text-cyan-700 transition-colors"
                >
                  Change Role
                </button>
              )}
              {member.status === "pending" && (
                <button
                  onClick={onResendInvite}
                  className="text-xs font-medium text-cyan-600 hover:text-cyan-700 transition-colors"
                >
                  Resend
                </button>
              )}
              <button
                onClick={onRemove}
                className="text-xs font-medium text-red-600 hover:text-red-700 transition-colors"
              >
                {member.status === "pending" ? "Cancel" : "Remove"}
              </button>
            </div>
          ) : (
            <span className="text-sm text-slate-300">&mdash;</span>
          )}
        </td>
      )}
    </tr>
  );
}
