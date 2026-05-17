import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { AuthProvider } from './hooks/useAuth';
import { ProtectedRoute } from './components/Layout';
import {
  Login,
  Dashboard,
  History,
  Settings,
  EthereumWallets,
  EthereumTransfer,
  EthereumRpcSettings,
  ZcashWallets,
  ZcashTransfer,
  ZcashRpcSettings,
  // F2.1
  MyPendingApprovals,
  ApprovalQueue,
  ApprovalDetail,
  ApprovalHistory,
  ApprovalPolicies,
  // F1.1
  AuditorDashboard,
  AuditorWalletDetail,
  DisclosureNew,
  AuditorList,
  AuditorLogin,
  // F3.1
  PayrollRunList,
  PayrollRunCreate,
  PayrollRunDetail,
  PayrollEmployees,
} from './pages';

function App() {
  return (
    <AuthProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route
            path="/"
            element={
              <ProtectedRoute>
                <Dashboard />
              </ProtectedRoute>
            }
          />
          {/* Legacy routes - redirect to Ethereum */}
          <Route path="/wallets" element={<Navigate to="/ethereum/wallets" replace />} />
          <Route path="/transfer" element={<Navigate to="/ethereum/transfer" replace />} />
          <Route
            path="/history"
            element={
              <ProtectedRoute>
                <History />
              </ProtectedRoute>
            }
          />
          <Route
            path="/settings"
            element={
              <ProtectedRoute>
                <Settings />
              </ProtectedRoute>
            }
          />

          {/* Ethereum Routes */}
          <Route
            path="/ethereum/wallets"
            element={
              <ProtectedRoute>
                <EthereumWallets />
              </ProtectedRoute>
            }
          />
          <Route
            path="/ethereum/transfer"
            element={
              <ProtectedRoute>
                <EthereumTransfer />
              </ProtectedRoute>
            }
          />
          <Route
            path="/ethereum/rpc"
            element={
              <ProtectedRoute>
                <EthereumRpcSettings />
              </ProtectedRoute>
            }
          />

          {/* Zcash Routes */}
          <Route
            path="/zcash/wallets"
            element={
              <ProtectedRoute>
                <ZcashWallets />
              </ProtectedRoute>
            }
          />
          <Route
            path="/zcash/transfer"
            element={
              <ProtectedRoute>
                <ZcashTransfer />
              </ProtectedRoute>
            }
          />
          <Route
            path="/zcash/rpc"
            element={
              <ProtectedRoute>
                <ZcashRpcSettings />
              </ProtectedRoute>
            }
          />

          {/* F2.1 — Maker/Checker approval routes.
              Queue + policies are admin-only (checker side);
              pending/detail/history available to operator + admin. */}
          <Route path="/approval/queue" element={<ProtectedRoute requiredRole="admin"><ApprovalQueue /></ProtectedRoute>} />
          <Route path="/approval/pending" element={<ProtectedRoute requiredRole={["admin", "operator"]}><MyPendingApprovals /></ProtectedRoute>} />
          <Route path="/approval/policies" element={<ProtectedRoute requiredRole="admin"><ApprovalPolicies /></ProtectedRoute>} />
          <Route path="/approval/:id" element={<ProtectedRoute requiredRole={["admin", "operator"]}><ApprovalDetail /></ProtectedRoute>} />
          <Route path="/approval/:id/history" element={<ProtectedRoute><ApprovalHistory /></ProtectedRoute>} />

          {/* F1.1 — Auditor / Viewing Key / Disclosure routes.
              Admin manages exports; Auditor logs in via separate /auditor/login. */}
          <Route path="/auditor/login" element={<AuditorLogin />} />
          <Route path="/auditor" element={<ProtectedRoute requiredRole={["admin", "auditor"]}><AuditorDashboard /></ProtectedRoute>} />
          <Route path="/auditor/manage" element={<ProtectedRoute requiredRole="admin"><AuditorList /></ProtectedRoute>} />
          <Route path="/auditor/wallets/:id" element={<ProtectedRoute requiredRole={["admin", "auditor"]}><AuditorWalletDetail /></ProtectedRoute>} />
          <Route path="/auditor/disclosure/new" element={<ProtectedRoute requiredRole={["admin", "auditor"]}><DisclosureNew /></ProtectedRoute>} />

          {/* F3.1 — Payroll routes. Create requires operator+admin;
              list/detail/employees viewable by all authenticated users. */}
          <Route path="/payroll/runs" element={<ProtectedRoute><PayrollRunList /></ProtectedRoute>} />
          <Route path="/payroll/runs/new" element={<ProtectedRoute requiredRole={["admin", "operator"]}><PayrollRunCreate /></ProtectedRoute>} />
          <Route path="/payroll/runs/:id" element={<ProtectedRoute><PayrollRunDetail /></ProtectedRoute>} />
          <Route path="/payroll/employees" element={<ProtectedRoute requiredRole={["admin", "operator"]}><PayrollEmployees /></ProtectedRoute>} />

          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}

export default App;
