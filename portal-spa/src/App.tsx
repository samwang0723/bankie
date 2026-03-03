import { BrowserRouter, Routes, Route, Navigate } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { AuthProvider } from "./hooks/useAuth.ts";
import { ErrorBoundary } from "./components/ErrorBoundary.tsx";
import { ProtectedRoute } from "./components/ProtectedRoute.tsx";
import { Layout } from "./components/Layout.tsx";
import { Login } from "./pages/Login.tsx";
import { Signup } from "./pages/Signup.tsx";
import { Dashboard } from "./pages/Dashboard.tsx";
import { ApiKeys } from "./pages/ApiKeys.tsx";
import { Organization } from "./pages/Organization.tsx";
import { ApiDocs } from "./pages/ApiDocs.tsx";
import { Accounts } from "./pages/Accounts.tsx";
import { Transactions } from "./pages/Transactions.tsx";
import { Reports } from "./pages/Reports.tsx";
import { Members } from "./pages/Members.tsx";
import { AcceptInvite } from "./pages/AcceptInvite.tsx";
import { Webhooks } from "./pages/Webhooks.tsx";
import { Logs } from "./pages/Logs.tsx";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
      refetchOnWindowFocus: false
    }
  }
});

export default function App() {
  return (
    <ErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <BrowserRouter>
          <AuthProvider>
            <Routes>
              {/* Public routes */}
              <Route path="/login" element={<Login />} />
              <Route path="/signup" element={<Signup />} />
              <Route path="/invite" element={<AcceptInvite />} />

              {/* Protected routes */}
              <Route
                element={
                  <ProtectedRoute>
                    <Layout />
                  </ProtectedRoute>
                }
              >
                <Route index element={<Dashboard />} />
                <Route path="api-keys" element={<ApiKeys />} />
                <Route path="organization" element={<Organization />} />
                <Route path="members" element={<Members />} />
                <Route path="api-docs" element={<ApiDocs />} />
                <Route path="accounts" element={<Accounts />} />
                <Route path="transactions" element={<Transactions />} />
                <Route path="reports" element={<Reports />} />
                <Route path="webhooks" element={<Webhooks />} />
                <Route path="logs" element={<Logs />} />
              </Route>

              {/* Catch-all */}
              <Route path="*" element={<Navigate to="/" replace />} />
            </Routes>
          </AuthProvider>
        </BrowserRouter>
      </QueryClientProvider>
    </ErrorBoundary>
  );
}
