import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';
import { Plus, RefreshCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { batchTransferService } from '../../services/api/batch-transfer';
import type { BatchTransferRun } from '../../types/batch-transfer';

/** F4.2 — Batch privacy transfer run list. */
export function BatchTransferRunList() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<BatchTransferRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      setRuns(await batchTransferService.listRuns());
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void load(); }, []);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('batch.list.title')}</h1>
        <div className="flex gap-2">
          <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
            <RefreshCw className="w-4 h-4" />
          </button>
          <button className="btn-primary" onClick={() => navigate('/batch-transfers/new')}>
            <Plus className="w-4 h-4 inline mr-1" /> {t('batch.list.new')}
          </button>
        </div>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-gray-500 uppercase text-xs">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('batch.list.col.title')}</th>
              <th className="text-left p-2">{t('batch.list.col.wallet')}</th>
              <th className="text-left p-2">{t('batch.list.col.privacy')}</th>
              <th className="text-right p-2">{t('batch.list.col.recipients')}</th>
              <th className="text-right p-2">{t('batch.list.col.total')}</th>
              <th className="text-left p-2">{t('batch.list.col.status')}</th>
              <th className="text-left p-2">{t('batch.list.col.created')}</th>
            </tr>
          </thead>
          <tbody>
            {runs.map(run => (
              <tr
                key={run.id}
                className="border-t border-gray-100 hover:bg-gray-50 cursor-pointer"
                onClick={() => navigate(`/batch-transfers/${run.id}`)}
              >
                <td className="p-2 font-mono">
                  <Link to={`/batch-transfers/${run.id}`} className="text-blue-600">#{run.id}</Link>
                </td>
                <td className="p-2">{run.title}</td>
                <td className="p-2">#{run.source_wallet_id}</td>
                <td className="p-2">{t(`batch.create.mode_${run.privacy_mode}`)}</td>
                <td className="p-2 text-right">{run.item_count}</td>
                <td className="p-2 text-right font-mono">{Number(run.total_amount)}</td>
                <td className="p-2">
                  <span className="px-2 py-0.5 rounded bg-gray-100 text-gray-800 text-xs">
                    {t(`batch.run_status.${run.status}`)}
                  </span>
                </td>
                <td className="p-2 text-gray-500">{new Date(run.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {runs.length === 0 && (
              <tr><td colSpan={8} className="p-6 text-center text-gray-400">{t('batch.list.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}
