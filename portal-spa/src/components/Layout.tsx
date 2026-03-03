import { useState } from "react";
import { Outlet } from "react-router-dom";
import { Menu } from "lucide-react";
import { Sidebar } from "./Sidebar.tsx";
import { ToastProvider } from "../hooks/useToast.tsx";

export function Layout() {
  const [sidebarOpen, setSidebarOpen] = useState(false);

  return (
    <ToastProvider>
      <div className="flex h-screen bg-[#F8FAFC]">
        <Sidebar isOpen={sidebarOpen} onClose={() => setSidebarOpen(false)} />

        <div className="flex-1 flex flex-col min-w-0">
          {/* Mobile hamburger - only visible on small screens */}
          <div className="lg:hidden flex items-center h-14 px-4 shrink-0">
            <button
              className="p-2 text-slate-600 hover:text-slate-900"
              onClick={() => setSidebarOpen(true)}
              aria-label="Open sidebar"
            >
              <Menu className="w-6 h-6" />
            </button>
          </div>

          {/* Main content */}
          <main className="flex-1 overflow-auto p-8 px-10">
            <Outlet />
          </main>
        </div>
      </div>
    </ToastProvider>
  );
}
