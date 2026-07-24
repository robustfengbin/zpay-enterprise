import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useNavigate } from 'react-router-dom';
import { Check, Play, RefreshCw, RotateCw, X, XCircle } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { migrationService } from '../../services/api/migration';
import type { ExecuteMigrationOutcome, MigrationRunSummary } from '../../types/migration';

/** Poll cadence while a run is live (executor ticks every 30 s). */
const POLL_MS = 10_000;

/**
 * F4.1 — Migration run detail / progress page. Polling-first: while the run
 * is executing or a private schedule is armed, refresh every 10 s so batch
 * status and the schedule timeline stay current without WS plumbing.
 */
export function MigrationRunDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [summary, setSummary] = useState<MigrationRunSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExecuteMigrationOutcome | null>(null);
  const [rejectReason, setRejectReason] = useState('');
  const [showReject, setShowReject] = useState(false);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const load = useCallback(async () => {
    try {
      setSummary(await migrationService.getRun(Number(id)));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => { void load(); }, [load]);

  // Live polling only while the run can still change on its own.
  useEffect(() => {
    const live = summary && ['approved', 'executing'].includes(summary.run.status);
    if (live && !timer.current) {
      timer.current = setInterval(() => void load(), POLL_MS);
    }
    if (!live && timer.current) {
      clearInterval(timer.current);
      timer.current = null;
    }
    return () => {
      if (timer.current) { clearInterval(timer.current); timer.current = null; }
    };
  }, [summary, load]);

  async function action(fn: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  async function onExecute() {
    setBusy(true);
    setError(null);
    try {
      const res = await migrationService.executeRun(Number(id));
      setOutcome(res);
      await load();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  if (loading) return <LoadingSpinner />;
  if (!summary) return <div className="p-6 text-red-600">{error || t('common.not_found')}</div>;

  const { run, items } = summary;
  const canExecute = run.status === 'pending' || run.status === 'approved';
  const canDecide = run.status === 'awaiting_approval';
  const canCancel = ['pending', 'awaiting_approval', 'approved', 'executing'].includes(run.status);
  const failed = items.filter(it => it.status === 'failed');
  const submitted = items.filter(it => it.status === 'submitted').length;
  const progressPct = items.length > 0 ? Math.round((submitted / items.length) * 100) : 0;

  return (
    <div className="p-6 max-w-6xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('migration.detail.title')} #{run.id}</h1>
          <p className="text-sm text-gray-500">
            {t('migration.detail.wallet')} #{run.source_wallet_id} · {t(`migration.create.mode_${run.mode}`)}
            {run.mode === 'private' && ` · ${run.batch_count} × ${run.window_hours}h`}
          </p>
        </div>
        <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      <Card>
        <dl className="grid grid-cols-4 gap-y-2 text-sm">
          <dt className="text-gray-500">{t('migration.detail.total')}</dt>
          <dd className="font-mono">{Number(run.total_amount)} ZEC</dd>
          <dt className="text-gray-500">{t('migration.detail.status')}</dt>
          <dd>
            <span className="px-2 py-0.5 rounded bg-gray-100 text-gray-800 text-xs">
              {t(`migration.run_status.${run.status}`)}
            </span>
          </dd>
          <dt className="text-gray-500">{t('migration.detail.progress')}</dt>
          <dd>{submitted}/{run.item_count}</dd>
          <dt className="text-gray-500">{t('migration.detail.created_by')}</dt>
          <dd>user#{run.created_by_user_id}</dd>
          {run.approved_by_user_id && (
            <>
              <dt className="text-gray-500">{t('migration.detail.approved_by')}</dt>
              <dd>user#{run.approved_by_user_id}</dd>
            </>
          )}
          {run.reject_reason && (
            <>
              <dt className="text-gray-500">{t('migration.detail.reject_reason')}</dt>
              <dd className="col-span-3 text-red-700">{run.reject_reason}</dd>
            </>
          )}
          {run.notes && (
            <>
              <dt className="text-gray-500">{t('migration.detail.notes')}</dt>
              <dd className="col-span-3 whitespace-pre-wrap text-gray-700">{run.notes}</dd>
            </>
          )}
        </dl>

        {(run.status === 'executing' || run.status === 'partial' || run.status === 'completed') && (
          <div className="mt-3">
            <div className="h-2 rounded bg-gray-100 overflow-hidden">
              <div className="h-2 bg-blue-600 transition-all" style={{ width: `${progressPct}%` }} />
            </div>
            <p className="text-xs text-gray-500 mt-1">{progressPct}%</p>
          </div>
        )}
      </Card>

      {outcome && outcome.result === 'awaiting_approval' && (
        <Card>
          <p className="text-sm text-yellow-800">
            {t('migration.detail.outcome_awaiting', {
              threshold: outcome.threshold,
              policy_id: outcome.policy_id,
            })}
          </p>
        </Card>
      )}

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <div className="flex gap-2 flex-wrap">
        {canExecute && (
          <button className="btn-primary" disabled={busy} onClick={onExecute}>
            <Play className="w-4 h-4 inline mr-1" />
            {run.mode === 'private'
              ? t('migration.detail.action.arm_schedule')
              : t('migration.detail.action.execute')}
          </button>
        )}
        {canDecide && (
          <>
            <button
              className="btn-primary"
              disabled={busy}
              onClick={() => action(() => migrationService.approveRun(run.id))}
            >
              <Check className="w-4 h-4 inline mr-1" /> {t('migration.detail.action.approve')}
            </button>
            <button className="btn-secondary" disabled={busy} onClick={() => setShowReject(v => !v)}>
              <X className="w-4 h-4 inline mr-1" /> {t('migration.detail.action.reject')}
            </button>
          </>
        )}
        {failed.length > 0 && run.status !== 'canceled' && (
          <button
            className="btn-secondary"
            disabled={busy}
            onClick={() => action(async () => {
              for (const it of failed) {
                await migrationService.retryItem(run.id, it.id);
              }
            })}
          >
            <RotateCw className="w-4 h-4 inline mr-1" />
            {t('migration.detail.action.retry_failed', { count: failed.length })}
          </button>
        )}
        {canCancel && (
          <button
            className="btn-ghost text-red-600"
            disabled={busy}
            onClick={() => {
              if (!confirm(t('migration.detail.confirm_cancel'))) return;
              void action(() => migrationService.cancelRun(run.id));
            }}
          >
            <XCircle className="w-4 h-4 inline mr-1" /> {t('migration.detail.action.cancel')}
          </button>
        )}
        <button className="btn-ghost" onClick={() => navigate('/migrations')}>
          {t('migration.detail.back_to_list')}
        </button>
      </div>

      {showReject && canDecide && (
        <Card>
          <label className="block text-sm text-gray-600 mb-1">{t('migration.detail.reject_reason_label')}</label>
          <div className="flex gap-2">
            <input
              className="input flex-1"
              value={rejectReason}
              onChange={e => setRejectReason(e.target.value)}
              placeholder={t('migration.detail.reject_reason_placeholder')}
            />
            <button
              className="btn-secondary"
              disabled={busy || rejectReason.trim().length < 5}
              onClick={() => action(async () => {
                await migrationService.rejectRun(run.id, rejectReason.trim());
                setShowReject(false);
              })}
            >
              {t('migration.detail.action.confirm_reject')}
            </button>
          </div>
        </Card>
      )}

      <Card>
        <h2 className="font-semibold mb-2 text-sm">{t('migration.detail.batches')}</h2>
        <table className="w-full text-xs">
          <thead className="text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-right p-2">{t('migration.detail.col.amount')}</th>
              <th className="text-left p-2">{t('migration.detail.col.scheduled')}</th>
              <th className="text-left p-2">{t('migration.detail.col.status')}</th>
              <th className="text-left p-2">{t('migration.detail.col.tx')}</th>
              <th className="text-left p-2">{t('migration.detail.col.reason')}</th>
            </tr>
          </thead>
          <tbody>
            {items.map(item => (
              <tr key={item.id} className="border-t border-gray-100">
                <td className="p-2 font-mono">{item.seq + 1}</td>
                <td className="p-2 text-right font-mono">{Number(item.amount)}</td>
                <td className="p-2">
                  {item.scheduled_at
                    ? new Date(item.scheduled_at).toLocaleString()
                    : t('migration.detail.immediately')}
                </td>
                <td className="p-2">
                  <span className={`text-xs px-1.5 py-0.5 rounded ${itemStatusColor(item.status)}`}>
                    {t(`migration.item_status.${item.status}`)}
                  </span>
                </td>
                <td className="p-2 font-mono truncate max-w-[180px]" title={item.tx_hash || ''}>
                  {item.tx_hash || '—'}
                </td>
                <td className="p-2 text-red-700 text-xs truncate max-w-[220px]" title={item.error_message || ''}>
                  {item.error_message || '—'}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function itemStatusColor(s: string): string {
  switch (s) {
    case 'submitted': return 'bg-green-100 text-green-800';
    case 'failed':    return 'bg-red-100 text-red-800';
    case 'canceled':  return 'bg-gray-200 text-gray-600';
    default:          return 'bg-gray-100 text-gray-700';
  }
}
