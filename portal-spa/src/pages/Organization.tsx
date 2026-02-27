import { useState, useEffect, type FormEvent } from 'react';
import { Trash2 } from 'lucide-react';
import { useAuth } from '../hooks/useAuth.ts';

export function Organization() {
  const { organization, user } = useAuth();

  const [name, setName] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (organization) {
      setName(organization.name);
    }
  }, [organization]);

  function handleSave(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    // TODO: wire up PATCH /portal/v1/orgs/:id
    setTimeout(() => setSaving(false), 500);
  }

  return (
    <div>
      {/* Page header */}
      <div className="mb-6">
        <h1 className="text-2xl font-bold text-slate-900">Organization</h1>
        <p className="mt-1 text-sm text-slate-500">
          Manage your organization details and settings.
        </p>
      </div>

      {/* Organization Details card */}
      <div className="bg-white rounded-xl border border-slate-200 p-7 mb-6">
        <h2 className="text-base font-semibold text-slate-900 mb-6">Organization Details</h2>

        <form onSubmit={handleSave}>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-5 mb-6">
            {/* Organization Name */}
            <div>
              <label htmlFor="org-name" className="block text-sm font-medium text-slate-500 mb-1.5">
                Organization Name
              </label>
              <input
                id="org-name"
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-900
                  focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
              />
            </div>

            {/* Slug */}
            <div>
              <label htmlFor="org-slug" className="block text-sm font-medium text-slate-500 mb-1.5">
                Slug
              </label>
              <input
                id="org-slug"
                type="text"
                value={organization?.slug ?? ''}
                readOnly
                className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-500
                  font-mono cursor-not-allowed"
              />
            </div>

            {/* Owner Email */}
            <div>
              <label htmlFor="org-email" className="block text-sm font-medium text-slate-500 mb-1.5">
                Owner Email
              </label>
              <input
                id="org-email"
                type="email"
                value={user?.email ?? ''}
                readOnly
                className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg text-sm text-slate-900
                  cursor-not-allowed"
              />
            </div>

            {/* Status */}
            <div>
              <label className="block text-sm font-medium text-slate-500 mb-1.5">
                Status
              </label>
              <div className="w-full h-11 px-3.5 bg-[#F8FAFC] border border-slate-200 rounded-lg flex items-center gap-2">
                <span className="w-2 h-2 rounded-full bg-green-500" />
                <span className="text-sm font-medium text-green-600">Active</span>
              </div>
            </div>
          </div>

          {/* Button row */}
          <div className="flex justify-end gap-3">
            <button
              type="button"
              onClick={() => setName(organization?.name ?? '')}
              className="h-10 px-5 text-sm font-medium text-slate-700 bg-slate-100 rounded-lg
                hover:bg-slate-200 transition-colors"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={saving || name === organization?.name}
              className="h-10 px-5 text-sm font-semibold text-[#0A0F1C] bg-cyan-400 rounded-lg
                hover:bg-cyan-500 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {saving ? 'Saving...' : 'Save Changes'}
            </button>
          </div>
        </form>
      </div>

      {/* Danger Zone */}
      <div className="bg-white rounded-xl border border-red-300 p-7">
        <h2 className="text-base font-semibold text-red-600 mb-3">Danger Zone</h2>
        <p className="text-sm text-slate-500 mb-4">
          Permanently delete this organization and all associated data. This action cannot be undone.
        </p>
        <button
          className="flex items-center gap-2 h-10 px-5 text-sm font-semibold text-white bg-red-600 rounded-lg
            hover:bg-red-700 transition-colors"
        >
          <Trash2 className="w-4 h-4" />
          Delete Organization
        </button>
      </div>
    </div>
  );
}
