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
  DisclosureHistory,
  DisclosureDetail,
  AuditorList,
  AuditorLogin,
  // F3.1
  PayrollRunList,
  PayrollRunCreate,
  PayrollRunDetail,
  PayrollEmployees,
  // F4.1
  MigrationRunList,
  MigrationRunCreate,
  MigrationRunDetail,
  // F4.2
  BatchTransferRunList,
  BatchTransferRunCreate,
  BatchTransferRunDetail,
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
          {/* Auditor side — strict dual-JWT isolation (PRD-F1.1 §3.1 NFR).
              Admin tokens cannot authenticate against the AuditorAuthMiddleware;
              admins manage auditors at /auditor/manage and sign out + sign in
              via /auditor/login if they need to preview the auditor's own
              dashboard. */}
          <Route path="/auditor/login" element={<AuditorLogin />} />
          <Route path="/auditor" element={<ProtectedRoute requiredRole="auditor"><AuditorDashboard /></ProtectedRoute>} />
          <Route path="/auditor/manage" element={<ProtectedRoute requiredRole="admin"><AuditorList /></ProtectedRoute>} />
          <Route path="/auditor/wallets/:id" element={<ProtectedRoute requiredRole="auditor"><AuditorWalletDetail /></ProtectedRoute>} />
          <Route path="/auditor/wallets/:walletId/disclosures" element={<ProtectedRoute requiredRole="auditor"><DisclosureHistory /></ProtectedRoute>} />
          <Route path="/auditor/disclosure/new" element={<ProtectedRoute requiredRole="auditor"><DisclosureNew /></ProtectedRoute>} />
          <Route path="/auditor/disclosure/:id" element={<ProtectedRoute requiredRole="auditor"><DisclosureDetail /></ProtectedRoute>} />

          {/* F3.1 — Payroll routes. Create requires operator+admin;
              list/detail/employees viewable by all authenticated users. */}
          <Route path="/payroll/runs" element={<ProtectedRoute><PayrollRunList /></ProtectedRoute>} />
          <Route path="/payroll/runs/new" element={<ProtectedRoute requiredRole={["admin", "operator"]}><PayrollRunCreate /></ProtectedRoute>} />
          <Route path="/payroll/runs/:id" element={<ProtectedRoute><PayrollRunDetail /></ProtectedRoute>} />
          <Route path="/payroll/employees" element={<ProtectedRoute requiredRole={["admin", "operator"]}><PayrollEmployees /></ProtectedRoute>} />

          {/* F4.1 — Orchard → Ironwood migration routes. Admin-only: a
              migration moves the whole shielded treasury. */}
          <Route path="/migrations" element={<ProtectedRoute requiredRole="admin"><MigrationRunList /></ProtectedRoute>} />
          <Route path="/migrations/new" element={<ProtectedRoute requiredRole="admin"><MigrationRunCreate /></ProtectedRoute>} />
          <Route path="/migrations/:id" element={<ProtectedRoute requiredRole="admin"><MigrationRunDetail /></ProtectedRoute>} />

          {/* F4.2 — generic batch privacy transfers. Admin-only: moves
              treasury funds to arbitrary recipients. */}
          <Route path="/batch-transfers" element={<ProtectedRoute requiredRole="admin"><BatchTransferRunList /></ProtectedRoute>} />
          <Route path="/batch-transfers/new" element={<ProtectedRoute requiredRole="admin"><BatchTransferRunCreate /></ProtectedRoute>} />
          <Route path="/batch-transfers/:id" element={<ProtectedRoute requiredRole="admin"><BatchTransferRunDetail /></ProtectedRoute>} />

          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </BrowserRouter>
    </AuthProvider>
  );
}

export default App;
