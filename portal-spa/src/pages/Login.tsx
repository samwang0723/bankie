import { useState, type FormEvent } from 'react';
import { Link, useNavigate, useLocation } from 'react-router-dom';
import { Landmark } from 'lucide-react';
import { useAuth } from '../hooks/useAuth.ts';
import { ApiClientError } from '../api/client.ts';

export function Login() {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);

  const { login } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();

  const from = (location.state as { from?: { pathname: string } })?.from?.pathname ?? '/';

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError('');
    setLoading(true);

    try {
      await login({ email, password });
      navigate(from, { replace: true });
    } catch (err) {
      if (err instanceof ApiClientError) {
        setError(err.apiError.message);
      } else {
        setError('An unexpected error occurred');
      }
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-[#0A0F1C]">
      <div className="w-full max-w-[420px] mx-4">
        <div className="bg-[#1E293B] rounded-xl p-10">
          {/* Logo */}
          <div className="flex flex-col items-center gap-2 mb-8">
            <Landmark className="w-10 h-10 text-cyan-400" />
            <span className="font-mono text-[28px] font-bold text-white">Bankie</span>
            <span className="text-sm text-slate-400">Developer Portal</span>
          </div>

          {error && (
            <div
              className="mb-4 p-3 bg-red-900/30 border border-red-800 text-red-400 rounded-md text-sm"
              role="alert"
            >
              {error}
            </div>
          )}

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <label
                htmlFor="email"
                className="block text-sm font-medium text-slate-300 mb-1.5"
              >
                Email
              </label>
              <input
                id="email"
                type="email"
                required
                autoComplete="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className="w-full px-3.5 py-2.5 bg-[#0F172A] border border-slate-600 rounded-lg text-sm text-white
                  placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
                placeholder="you@company.com"
              />
            </div>

            <div>
              <label
                htmlFor="password"
                className="block text-sm font-medium text-slate-300 mb-1.5"
              >
                Password
              </label>
              <input
                id="password"
                type="password"
                required
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full px-3.5 py-2.5 bg-[#0F172A] border border-slate-600 rounded-lg text-sm text-white
                  placeholder:text-slate-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:border-cyan-400"
                placeholder="Enter your password"
              />
            </div>

            <button
              type="submit"
              disabled={loading}
              className="w-full h-11 bg-cyan-400 text-[#0A0F1C] rounded-lg text-[15px] font-semibold
                hover:bg-cyan-500 focus:outline-none focus:ring-2 focus:ring-cyan-400 focus:ring-offset-2
                focus:ring-offset-[#1E293B] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {loading ? 'Signing in...' : 'Sign In'}
            </button>
          </form>

          {/* Divider */}
          <div className="flex items-center gap-3 my-6">
            <div className="flex-1 h-px bg-slate-600" />
            <span className="font-mono text-xs text-slate-500 tracking-widest">OR</span>
            <div className="flex-1 h-px bg-slate-600" />
          </div>

          {/* Create Account button */}
          <Link
            to="/signup"
            className="flex items-center justify-center w-full h-11 border border-slate-600 rounded-lg
              text-[15px] font-semibold text-cyan-400 hover:bg-slate-800/50 transition-colors"
          >
            Create Account
          </Link>
        </div>
      </div>
    </div>
  );
}
