import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, FileText, RefreshCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type { DisclosureRow } from '../../types/viewing-key';

/**
 * F1.1 §5 — Per-wallet disclosure history. Lists past + in-flight
 * disclosures for a single wallet so an admin / auditor can pick up
 * where they left off (e.g. failed jobs to retry, generating ones to
 * watch).
 */
export function DisclosureHistory() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { walletId } = useParams<{ walletId: string }>();
  const wid = Number(walletId);

  const [rows, setRows] = useState<DisclosureRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    if (!Number.isFinite(wid)) { setError(t('common.not_found')); setLoading(false); return; }
    setLoading(true);
    try {
      setRows(await viewingKeyService.listDisclosures(wid));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void load(); }, [wid]);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <button className="btn-ghost mb-2" onClick={() => navigate(`/auditor/wallets/${wid}`)}>
            <ArrowLeft className="w-4 h-4 inline mr-1" /> {t('auditor.disclosure.history_back_wallet')}
          </button>
          <h1 className="text-2xl font-semibold">{t('auditor.disclosure.history_title')} · wallet #{wid}</h1>
        </div>
        <div className="flex gap-2">
          <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
            <RefreshCw className="w-4 h-4" />
          </button>
          <button className="btn-primary" onClick={() => navigate(`/auditor/disclosure/new?wallet_id=${wid}`)}>
            <FileText className="w-4 h-4 inline mr-1" /> {t('auditor.disclosure.history_new')}
          </button>
        </div>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.granularity')}</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.scope_param')}</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.format')}</th>
              <th className="text-right p-2">{t('auditor.disclosure.col.tx_count')}</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.status')}</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.created_at')}</th>
              <th className="text-left p-2">{t('auditor.disclosure.col.expires_at')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(r => (
              <tr
                key={r.id}
                className="border-t border-gray-100 hover:bg-gray-50 cursor-pointer"
                onClick={() => navigate(`/auditor/disclosure/${r.id}`)}
              >
                <td className="p-2 font-mono">{r.id}</td>
                <td className="p-2">{t(`auditor.disclosure.granularity.${r.granularity as 'tx' | 'address' | 'range'}`)}</td>
                <td className="p-2 font-mono text-xs truncate max-w-[200px]" title={JSON.stringify(r.scope_param)}>
                  {summarizeScope(r.scope_param, r.granularity)}
                </td>
                <td className="p-2 uppercase text-xs">{r.format}</td>
                <td className="p-2 text-right">{r.tx_count}</td>
                <td className="p-2">
                  <span className={`text-xs px-1.5 py-0.5 rounded ${statusClass(r.status)}`}>
                    {t(`auditor.disclosure.status_${r.status}`)}
                  </span>
                </td>
                <td className="p-2 text-xs">{new Date(r.created_at).toLocaleString()}</td>
                <td className="p-2 text-xs">{new Date(r.expires_at).toLocaleString()}</td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr><td colSpan={8} className="p-4 text-center text-gray-400">{t('auditor.disclosure.history_empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function statusClass(s: string): string {
  switch (s) {
    case 'ready':      return 'bg-green-100 text-green-800';
    case 'generating': return 'bg-blue-100 text-blue-800';
    case 'failed':     return 'bg-red-100 text-red-800';
    default:           return 'bg-gray-100 text-gray-700';
  }
}

function summarizeScope(scope: Record<string, unknown>, granularity: string): string {
  if (granularity === 'tx') return String(scope.tx_hash || '').slice(0, 16) + '…';
  if (granularity === 'address') return String(scope.address || '').slice(0, 20) + '…';
  if (granularity === 'range') return `${scope.from} → ${scope.to}`;
  return JSON.stringify(scope);
}
