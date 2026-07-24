import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Eye, FileText, Key, ChevronRight } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { ViewingKeyExportModal } from './ViewingKeyExport';
import { viewingKeyService } from '../../services/api';
import type { AuditorTenantSummary } from '../../types/viewing-key';
import { useAuth } from '../../hooks/useAuth';

/**
 * F1.1 §2 Auditor Dashboard — list of scoped wallets with scope window,
 * disclosure budget, and aggregate activity counters.
 *
 * Auditors click a wallet row to drill into balance + transfers within their
 * scope window. Raw on-chain amounts require a follow-up disclosure to decrypt
 * shielded data; balance/transfers come straight from the live RPC.
 *
 * Admins viewing this page also see an "Export viewing key" CTA so they can
 * hand a one-time download token to an external auditor via the modal flow.
 */
export function AuditorDashboard() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { user } = useAuth();
  const [wallets, setWallets] = useState<AuditorTenantSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [exportFor, setExportFor] = useState<AuditorTenantSummary | null>(null);

  const isAdmin = user?.role === 'admin';

  useEffect(() => {
    (async () => {
      try {
        setWallets(await viewingKeyService.listAuditorWallets());
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-4 max-w-6xl">
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
              <th className="text-left p-2">{t('auditor.dashboard.col.scope_window')}</th>
              <th className="text-right p-2">{t('auditor.dashboard.col.tx_count')}</th>
              <th className="text-left p-2">{t('auditor.dashboard.col.last_activity')}</th>
              <th className="text-right p-2">{t('auditor.dashboard.col.budget')}</th>
              <th className="text-right p-2">{t('auditor.dashboard.col.actions')}</th>
            </tr>
          </thead>
          <tbody>
            {wallets.map(w => {
              const budgetRemaining = Math.max(0, w.max_disclosure_count - w.current_count);
              const budgetExhausted = budgetRemaining === 0;
              return (
                <tr
                  key={w.wallet_id}
                  className="border-t border-gray-100 hover:bg-gray-50 cursor-pointer"
                  onClick={() => navigate(`/auditor/wallets/${w.wallet_id}`)}
                >
                  <td className="p-2">
                    <div className="font-medium">{w.wallet_name}</div>
                    <div className="font-mono text-xs text-gray-500 truncate max-w-xs" title={w.address}>{w.address}</div>
                  </td>
                  <td className="p-2">{w.chain}</td>
                  <td className="p-2 text-xs">
                    {new Date(w.scope_start).toLocaleDateString()} → {new Date(w.scope_end).toLocaleDateString()}
                  </td>
                  <td className="p-2 text-right">{w.total_tx_count}</td>
                  <td className="p-2 text-xs">
                    {w.last_activity_at ? new Date(w.last_activity_at).toLocaleString() : '—'}
                  </td>
                  <td className={`p-2 text-right text-xs ${budgetExhausted ? 'text-red-600' : ''}`}>
                    {w.current_count} / {w.max_disclosure_count}
                    {w.pending_disclosures > 0 && (
                      <span className="ml-1 text-yellow-700" title={t('auditor.dashboard.pending_disclosures')}>
                        +{w.pending_disclosures}⏳
                      </span>
                    )}
                  </td>
                  <td className="p-2 text-right space-x-1" onClick={e => e.stopPropagation()}>
                    {isAdmin && (
                      <button
                        className="btn-ghost"
                        onClick={() => setExportFor(w)}
                        title={t('auditor.dashboard.export_viewing_key')}
                      >
                        <Key className="w-4 h-4 inline" />
                      </button>
                    )}
                    {/* Disclosure creation is an admin capability (backend has
                        no auditor-side create endpoint) — visible but disabled
                        so the auditor knows the feature exists and who to ask. */}
                    <button
                      className="btn-secondary opacity-50 cursor-not-allowed"
                      disabled
                      title={t('auditor.disclosure.admin_only')}
                    >
                      <FileText className="w-4 h-4 inline mr-1" />
                      {t('auditor.dashboard.request_disclosure')}
                    </button>
                    <button
                      className="btn-ghost"
                      onClick={() => navigate(`/auditor/wallets/${w.wallet_id}`)}
                      title={t('auditor.dashboard.view_detail')}
                    >
                      <ChevronRight className="w-4 h-4 inline" />
                    </button>
                  </td>
                </tr>
              );
            })}
            {wallets.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('auditor.dashboard.empty')}</td></tr>
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
