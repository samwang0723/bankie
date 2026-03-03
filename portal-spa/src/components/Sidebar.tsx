import { NavLink } from "react-router-dom";
import {
  Landmark,
  LayoutDashboard,
  Key,
  Building2,
  Users,
  BookOpen,
  Wallet,
  ArrowLeftRight,
  FileText,
  Radio,
  ScrollText,
  ShieldCheck,
  LogOut
} from "lucide-react";
import { useAuth } from "../hooks/useAuth.ts";

const NAV_ITEMS = [
  { label: "Dashboard", path: "/", icon: LayoutDashboard },
  { label: "API Keys", path: "/api-keys", icon: Key },
  { label: "Organization", path: "/organization", icon: Building2 },
  { label: "Team", path: "/members", icon: Users },
  { label: "API Docs", path: "/api-docs", icon: BookOpen },
  { label: "Accounts", path: "/accounts", icon: Wallet },
  { label: "Transactions", path: "/transactions", icon: ArrowLeftRight },
  { label: "Reports", path: "/reports", icon: FileText },
  { label: "Webhooks", path: "/webhooks", icon: Radio },
  { label: "API Logs", path: "/logs", icon: ScrollText },
  { label: "Audit Logs", path: "/audit-logs", icon: ShieldCheck }
] as const;

interface SidebarProps {
  isOpen: boolean;
  onClose: () => void;
}

export function Sidebar({ isOpen, onClose }: SidebarProps) {
  const { logout } = useAuth();

  return (
    <>
      {/* Mobile overlay */}
      {isOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/50 lg:hidden"
          onClick={onClose}
          aria-hidden="true"
        />
      )}

      <aside
        className={`fixed inset-y-0 left-0 z-50 w-60 bg-[#0A0F1C] flex flex-col transform transition-transform duration-200 ease-in-out
          lg:translate-x-0 lg:static lg:z-auto ${
            isOpen ? "translate-x-0" : "-translate-x-full"
          }`}
        aria-label="Main navigation"
      >
        {/* Logo */}
        <div className="flex items-center gap-2.5 px-6 pt-6 pb-0">
          <Landmark className="w-6 h-6 text-cyan-400 shrink-0" />
          <span className="font-mono text-lg font-bold text-white">Bankie</span>
        </div>

        {/* Spacer */}
        <div className="h-8" />

        {/* Navigation */}
        <nav className="px-4 space-y-1">
          <p className="font-mono text-[10px] font-semibold text-slate-500 tracking-[2px] uppercase px-3 pb-2">
            NAVIGATION
          </p>
          {NAV_ITEMS.map((item) => (
            <NavLink
              key={item.label}
              to={item.path}
              end={item.path === "/"}
              onClick={onClose}
              className={({ isActive }) =>
                `flex items-center gap-2.5 px-3 h-10 rounded-lg text-sm transition-colors ${
                  isActive
                    ? "bg-[#1E293B] text-white font-semibold"
                    : "text-slate-400 hover:bg-slate-800/50"
                }`
              }
            >
              {({ isActive }) => (
                <>
                  <item.icon
                    className={`w-[18px] h-[18px] shrink-0 ${isActive ? "text-cyan-400" : ""}`}
                  />
                  {item.label}
                </>
              )}
            </NavLink>
          ))}
        </nav>

        {/* Flex spacer */}
        <div className="flex-1" />

        {/* Logout */}
        <div className="px-4 pb-6">
          <button
            onClick={logout}
            className="flex items-center gap-2.5 px-5 h-10 rounded-lg text-sm font-medium text-slate-500
              hover:text-slate-300 hover:bg-slate-800/50 transition-colors w-full"
            aria-label="Sign out"
          >
            <LogOut className="w-[18px] h-[18px] shrink-0" />
            Logout
          </button>
        </div>
      </aside>
    </>
  );
}
