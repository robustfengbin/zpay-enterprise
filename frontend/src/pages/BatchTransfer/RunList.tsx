import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw, Send, Shield } from 'lucide-react';
import {
  Amount,
  Card,
  EmptyState,
  PageHeader,
  RunStatusBadge,
  TimeAgo,
} from '../../components/Common';
import { TableSkeleton } from '../Migration/RunList';
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

  useEffect(() => {
    void load();
  }, []);

  return (
    <>
      <PageHeader
        title={t('batch.list.title')}
        subtitle={t('batch.list.subtitle')}
        actions={
          <>
            <button
              className="btn-secondary btn-icon"
              onClick={() => void load()}
              title={t('common.refresh')}
              aria-label={t('common.refresh')}
            >
              <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            </button>
            <button className="btn-primary" onClick={() => navigate('/batch-transfers/new')}>
              <Plus className="h-4 w-4" /> {t('batch.list.new')}
            </button>
          </>
        }
      />

      {error && <div className="alert alert-bad mb-4">{error}</div>}

      <Card flush>
        {loading ? (
          <TableSkeleton />
        ) : runs.length === 0 ? (
          <EmptyState
            icon={Send}
            title={t('batch.list.empty')}
            body={t('batch.list.empty_body')}
            action={
              <button className="btn-primary" onClick={() => navigate('/batch-transfers/new')}>
                <Plus className="h-4 w-4" /> {t('batch.list.new')}
              </button>
            }
          />
        ) : (
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>#</th>
                  <th>{t('batch.list.col.title')}</th>
                  <th>{t('batch.list.col.wallet')}</th>
                  <th>{t('batch.list.col.privacy')}</th>
                  <th className="cell-num">{t('batch.list.col.recipients')}</th>
                  <th className="cell-num">{t('batch.list.col.total')}</th>
                  <th>{t('batch.list.col.status')}</th>
                  <th>{t('batch.list.col.created')}</th>
                </tr>
              </thead>
              <tbody>
                {runs.map((run) => (
                  <tr
                    key={run.id}
                    className="row-link"
                    onClick={() => navigate(`/batch-transfers/${run.id}`)}
                  >
                    <td className="num font-medium text-brand-700">#{run.id}</td>
                    <td className="max-w-[220px] truncate font-medium text-ink-900" title={run.title}>
                      {run.title}
                    </td>
                    <td className="text-ink-500">
                      {t('batch.detail.wallet')} #{run.source_wallet_id}
                    </td>
                    <td>
                      <span
                        className={`badge ${
                          run.privacy_mode === 'staggered' ? 'badge-brand' : 'badge-neutral'
                        }`}
                      >
                        {run.privacy_mode === 'staggered' && <Shield className="h-3 w-3" />}
                        {t(`batch.create.mode_${run.privacy_mode}`)}
                      </span>
                    </td>
                    <td className="cell-num text-ink-700">{run.item_count}</td>
                    <td className="cell-num">
                      <Amount value={run.total_amount} />
                    </td>
                    <td>
                      <RunStatusBadge
                        status={run.status}
                        label={t(`batch.run_status.${run.status}`)}
                      />
                    </td>
                    <td className="text-ink-400">
                      <TimeAgo value={run.created_at} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </>
  );
}
