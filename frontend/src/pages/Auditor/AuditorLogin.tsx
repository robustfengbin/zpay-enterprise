import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Eye } from 'lucide-react';
import { auditorAuthService } from '../../services/api';

/**
 * Standalone auditor login page. Separate from the main /login because
 * auditor accounts use email (not username) and live in a different
 * table + JWT claim (`kind: "auditor"`). Stores the token under
 * localStorage `auditor_token` to avoid colliding with the admin/operator
 * token used by the rest of the app.
 *
 * Note: this page does NOT use the main Layout — auditor sessions don't
 * have access to the full wallet/transfer/payroll surface. Once logged in,
 * the auditor is redirected to /auditor (read-only scoped dashboard).
 */
export function AuditorLogin() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const resp = await auditorAuthService.login(email, password);
      // Keep auditor token separate from the main 'token' key used by useAuth.
      localStorage.setItem('auditor_token', resp.token);
      localStorage.setItem('auditor_session', JSON.stringify(resp.auditor));
      navigate('/auditor');
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50">
      <div className="bg-white rounded-lg shadow-md p-8 w-full max-w-sm">
        <div className="flex items-center justify-center mb-6">
          <Eye className="w-12 h-12 text-blue-600" />
        </div>
        <h1 className="text-xl font-semibold text-center">{t('auditor.login.title')}</h1>
        <p className="text-sm text-gray-500 text-center mt-1 mb-6">{t('auditor.login.subtitle')}</p>

        <form onSubmit={onSubmit} className="space-y-4">
          <label className="block">
            <span className="text-sm text-gray-600">{t('auditor.login.email')}</span>
            <input
              type="email"
              required
              autoComplete="email"
              className="mt-1 w-full border rounded p-2"
              value={email}
              onChange={e => setEmail(e.target.value)}
            />
          </label>
          <label className="block">
            <span className="text-sm text-gray-600">{t('auditor.login.password')}</span>
            <input
              type="password"
              required
              autoComplete="current-password"
              className="mt-1 w-full border rounded p-2"
              value={password}
              onChange={e => setPassword(e.target.value)}
            />
          </label>

          {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}

          <button type="submit" className="btn-primary w-full" disabled={submitting}>
            {submitting ? t('common.loading') : t('auditor.login.submit')}
          </button>
        </form>

        <p className="text-xs text-gray-500 text-center mt-4">
          {t('auditor.login.not_auditor')} <a href="/login" className="text-blue-600 hover:underline">{t('auditor.login.go_main')}</a>
        </p>
      </div>
    </div>
  );
}
