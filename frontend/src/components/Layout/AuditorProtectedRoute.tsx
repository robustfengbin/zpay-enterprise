import React from 'react';
import { Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Eye, LogOut } from 'lucide-react';
import type { AuditorSession } from '../../services/api/viewing-key';

/**
 * Route guard for the auditor portal (/auditor/*). Auditor sessions are
 * entirely separate from the main app's useAuth: they live under
 * localStorage `auditor_token` / `auditor_session` and authenticate against
 * the backend's AuditorAuthMiddleware (admin tokens are rejected there).
 * Deliberately does NOT render the main Layout — auditors have no access to
 * the wallet/transfer/payroll surface, so they get a minimal read-only shell.
 */
export function AuditorProtectedRoute({ children }: { children: React.ReactNode }) {
  const { t } = useTranslation();

  const token = localStorage.getItem('auditor_token');
  let session: AuditorSession | null = null;
  try {
    const raw = localStorage.getItem('auditor_session');
    session = raw ? (JSON.parse(raw) as AuditorSession) : null;
  } catch {
    localStorage.removeItem('auditor_session');
  }

  if (!token || !session) {
    return <Navigate to="/auditor/login" replace />;
  }

  function signOut() {
    localStorage.removeItem('auditor_token');
    localStorage.removeItem('auditor_session');
    window.location.href = '/auditor/login';
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <header className="bg-white border-b px-6 py-3 flex items-center justify-between">
        <div className="flex items-center gap-2 text-gray-800">
          <Eye className="w-5 h-5 text-blue-600" />
          <span className="font-semibold">{t('auditor.shell.title')}</span>
        </div>
        <div className="flex items-center gap-4">
          <span className="text-sm text-gray-600">{session.email}</span>
          <button className="btn-ghost flex items-center gap-1 text-sm" onClick={signOut}>
            <LogOut className="w-4 h-4" />
            {t('auditor.shell.sign_out')}
          </button>
        </div>
      </header>
      <main className="p-6 overflow-y-auto">{children}</main>
    </div>
  );
}
