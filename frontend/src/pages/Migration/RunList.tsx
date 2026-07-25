import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw, ShieldCheck, Zap } from 'lucide-react';
import {
  Amount,
  Card,
  EmptyState,
  PageHeader,
  RunStatusBadge,
  TimeAgo,
} from '../../components/Common';
import { migrationService } from '../../services/api/migration';
import type { MigrationRun } from '../../types/migration';

/** F4.1 — Migration run list. */
export function MigrationRunList() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<MigrationRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      setRuns(await migrationService.listRuns());
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
        title={t('migration.list.title')}
        subtitle={t('migration.list.subtitle')}
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
            <button className="btn-primary" onClick={() => navigate('/migrations/new')}>
              <Plus className="h-4 w-4" /> {t('migration.list.new')}
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
            icon={ShieldCheck}
            title={t('migration.list.empty')}
            body={t('migration.list.empty_body')}
            action={
              <button className="btn-primary" onClick={() => navigate('/migrations/new')}>
                <Plus className="h-4 w-4" /> {t('migration.list.new')}
              </button>
            }
          />
        ) : (
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>#</th>
                  <th>{t('migration.list.col.wallet')}</th>
                  <th>{t('migration.list.col.mode')}</th>
                  <th className="cell-num">{t('migration.list.col.batches')}</th>
                  <th className="cell-num">{t('migration.list.col.total')}</th>
                  <th>{t('migration.list.col.status')}</th>
                  <th>{t('migration.list.col.created')}</th>
                </tr>
              </thead>
              <tbody>
                {runs.map((run) => (
                  <tr
                    key={run.id}
                    className="row-link"
                    onClick={() => navigate(`/migrations/${run.id}`)}
                  >
                    <td className="num font-medium text-brand-700">#{run.id}</td>
                    <td className="text-ink-700">
                      {t('migration.detail.wallet')} #{run.source_wallet_id}
                    </td>
                    <td>
                      <span
                        className={`badge ${run.mode === 'private' ? 'badge-brand' : 'badge-neutral'}`}
                      >
                        {run.mode === 'private' ? (
                          <ShieldCheck className="h-3 w-3" />
                        ) : (
                          <Zap className="h-3 w-3" />
                        )}
                        {t(`migration.create.mode_${run.mode}`)}
                      </span>
                    </td>
                    <td className="cell-num text-ink-500">
                      {run.mode === 'private' ? `${run.batch_count} × ${run.window_hours}h` : '1'}
                    </td>
                    <td className="cell-num">
                      <Amount value={run.total_amount} />
                    </td>
                    <td>
                      <RunStatusBadge
                        status={run.status}
                        label={t(`migration.run_status.${run.status}`)}
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

/** Loading placeholder that keeps the table's shape instead of collapsing it. */
export function TableSkeleton({ rows = 3 }: { rows?: number }) {
  return (
    <div className="divide-y divide-line-100">
      {Array.from({ length: rows }).map((_, i) => (
        <div key={i} className="flex items-center gap-4 px-3.5 py-3.5">
          <div className="skeleton h-3.5 w-10" />
          <div className="skeleton h-3.5 flex-1" />
          <div className="skeleton h-3.5 w-24" />
          <div className="skeleton h-3.5 w-16" />
        </div>
      ))}
    </div>
  );
}
