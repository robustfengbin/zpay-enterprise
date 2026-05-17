import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Eye, FileText, Key } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { ViewingKeyExportModal } from './ViewingKeyExport';
import { viewingKeyService } from '../../services/api';
import type { AuditorTenantSummary } from '../../types/viewing-key';
import { useAuth } from '../../hooks/useAuth';

/**
 * F1.1 §2 Auditor Dashboard — read-only summary across tenants.
 * Auditor role sees only aggregate counts + last activity; to view actual
 * amounts they must request a payment disclosure (which is logged).
 *
 * Admins viewing this page also get an "Export viewing key" CTA on each
 * row so they can hand a one-time download token to an external auditor
 * via the modal flow. This entry-point is admin-only (gated by role check
 * below) — auditors never see it.
 */
export function AuditorDashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { user } = useAuth();
  const [tenants, setTenants] = useState<AuditorTenantSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [exportFor, setExportFor] = useState<AuditorTenantSummary | null>(null);

  const isAdmin = user?.role === 'admin';

  useEffect(() => {
    (async () => {
      try {
        setTenants(await viewingKeyService.listAuditorWallets());
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-4 max-w-5xl">
      <header>
        <h1 className="text-2xl font-semibold flex items-center gap-2">
          <Eye className="w-6 h-6" />
          {t('auditor.dashboard.title')}
        </h1>
        <p className="text-sm text-gray-500 mt-1">{t('auditor.dashboard.subtitle')}</p>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">{t('auditor.dashboard.col.wallet')}</th>
              <th className="text-left p-2">{t('auditor.dashboard.col.chain')}</th>
              <th className="text-left p-2">{t('auditor.dashboard.col.address')}</th>
              <th className="text-right p-2">{t('auditor.dashboard.col.tx_count')}</th>
              <th className="text-left p-2">{t('auditor.dashboard.col.last_activity')}</th>
              <th className="text-right p-2">{t('auditor.dashboard.col.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {tenants.map(t1 => (
              <tr key={t1.wallet_id} className="border-t border-gray-100 hover:bg-gray-50">
                <td className="p-2 font-medium">{t1.wallet_name}</td>
                <td className="p-2">{t1.chain}</td>
                <td className="p-2 font-mono text-xs truncate max-w-xs">{t1.address}</td>
                <td className="p-2 text-right">{t1.total_tx_count}</td>
                <td className="p-2 text-xs">{t1.last_activity_at ? new Date(t1.last_activity_at).toLocaleString() : '—'}</td>
                <td className="p-2 text-right space-x-1">
                  {isAdmin && (
                    <button
                      className="btn-ghost"
                      onClick={() => setExportFor(t1)}
                      title={t('auditor.dashboard.export_viewing_key')}
                    >
                      <Key className="w-4 h-4 inline" />
                    </button>
                  )}
                  <button
                    className="btn-secondary"
                    onClick={() => navigate(`/auditor/disclosure/new?wallet_id=${t1.wallet_id}`)}
                    title={t('auditor.dashboard.request_disclosure')}
                  >
                    <FileText className="w-4 h-4 inline mr-1" />
                    {t('auditor.dashboard.request_disclosure')}
                  </button>
                </td>
              </tr>
            ))}
            {tenants.length === 0 && (
              <tr><td colSpan={6} className="p-4 text-center text-gray-400">{t('auditor.dashboard.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>

      {exportFor && (
        <ViewingKeyExportModal
          walletId={exportFor.wallet_id}
          walletName={exportFor.wallet_name}
          open={true}
          onClose={() => setExportFor(null)}
        />
      )}
    </div>
  );
}
