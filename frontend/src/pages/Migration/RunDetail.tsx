import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import { Check, Play, RefreshCw, RotateCw, ShieldCheck, X, XCircle, Zap } from 'lucide-react';
import {
  Amount,
  Card,
  Hash,
  ItemStatusBadge,
  LoadingSpinner,
  PageHeader,
  PoolFlow,
  RunStatusBadge,
  Stat,
  StatRow,
  TimeAgo,
} from '../../components/Common';
import { migrationService } from '../../services/api/migration';
import orchardApi from '../../services/api/orchard';
import type { ExecuteMigrationOutcome, MigrationRunSummary } from '../../types/migration';
import type { ShieldedBalanceByPool } from '../../types/orchard';

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
  const [summary, setSummary] = useState<MigrationRunSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExecuteMigrationOutcome | null>(null);
  const [rejectReason, setRejectReason] = useState('');
  const [pools, setPools] = useState<ShieldedBalanceByPool | null>(null);
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const load = useCallback(async () => {
    try {
      const next = await migrationService.getRun(Number(id));
      setSummary(next);
      setError(null);
      // Real per-pool balances for the flow header. Advisory: if it fails the
      // page falls back to the run's own arithmetic below.
      orchardApi
        .getShieldedBalanceByPool(next.run.source_wallet_id)
        .then(setPools)
        .catch(() => setPools(null));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

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
      if (timer.current) {
        clearInterval(timer.current);
        timer.current = null;
      }
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
  if (!summary) return <div className="alert alert-bad">{error || t('common.not_found')}</div>;

  const { run, items } = summary;
  const canExecute = run.status === 'pending' || run.status === 'approved';
  const canDecide = run.status === 'awaiting_approval';
  const canCancel = ['pending', 'awaiting_approval', 'approved', 'executing'].includes(run.status);
  const failed = items.filter((it) => it.status === 'failed');
  const submitted = items.filter((it) => it.status === 'submitted');

  const total = Number(run.total_amount) || 0;
  const moved = submitted.reduce((acc, it) => acc + (Number(it.amount) || 0), 0);
  const pct = total > 0 ? (moved / total) * 100 : 0;

  // The two amounts are the wallet's real per-pool balances, so the header
  // answers "where is my money now" rather than "what does the plan say".
  // They differ: the plan reserves fee headroom it has not spent yet. Until
  // the balances load (or if they fail), fall back to the run's arithmetic.
  const poolAmount = (pool: string) =>
    (pools?.pools.find((p) => p.pool === pool)?.total_zatoshis ?? 0) / 1e8;
  const legacyAmount = pools ? poolAmount('orchard') : Math.max(0, total - moved);
  const ironwoodAmount = pools ? poolAmount('ironwood') : moved;
  const live = run.status === 'executing' || run.status === 'approved';

  return (
    <>
      <PageHeader
        backTo={{ to: '/migrations', label: t('migration.detail.back_to_list') }}
        title={`${t('migration.detail.title')} #${run.id}`}
        subtitle={`${t('migration.detail.wallet')} #${run.source_wallet_id}`}
        meta={
          <>
            <RunStatusBadge status={run.status} label={t(`migration.run_status.${run.status}`)} />
            <span className={`badge ${run.mode === 'private' ? 'badge-brand' : 'badge-neutral'}`}>
              {run.mode === 'private' ? (
                <ShieldCheck className="h-3 w-3" />
              ) : (
                <Zap className="h-3 w-3" />
              )}
              {t(`migration.create.mode_${run.mode}`)}
            </span>
            {run.mode === 'private' && (
              <span className="badge badge-neutral">
                {run.batch_count} × {run.window_hours}h
              </span>
            )}
          </>
        }
        actions={
          <>
            <button
              className="btn-secondary btn-icon"
              onClick={() => void load()}
              title={t('common.refresh')}
              aria-label={t('common.refresh')}
            >
              <RefreshCw className={`h-4 w-4 ${busy ? 'animate-spin' : ''}`} />
            </button>
            {failed.length > 0 && run.status !== 'canceled' && (
              <button
                className="btn-secondary"
                disabled={busy}
                onClick={() =>
                  action(async () => {
                    for (const it of failed) {
                      await migrationService.retryItem(run.id, it.id);
                    }
                  })
                }
              >
                <RotateCw className="h-4 w-4" />
                {t('migration.detail.action.retry_failed', { count: failed.length })}
              </button>
            )}
            {canCancel && (
              <button
                className="btn-secondary text-bad-600"
                disabled={busy}
                onClick={() => {
                  if (!confirm(t('migration.detail.confirm_cancel'))) return;
                  void action(() => migrationService.cancelRun(run.id));
                }}
              >
                <XCircle className="h-4 w-4" /> {t('migration.detail.action.cancel')}
              </button>
            )}
            {canExecute && (
              <button className="btn-primary" disabled={busy} onClick={onExecute}>
                <Play className="h-4 w-4" />
                {run.mode === 'private'
                  ? t('migration.detail.action.arm_schedule')
                  : t('migration.detail.action.execute')}
              </button>
            )}
          </>
        }
      />

      <div className="space-y-4">
        {error && <div className="alert alert-bad">{error}</div>}

        {outcome && outcome.result === 'awaiting_approval' && (
          <div className="alert alert-warn">
            {t('migration.detail.outcome_awaiting', {
              threshold: outcome.threshold,
              policy_id: outcome.policy_id,
            })}
          </div>
        )}

        {canDecide && (
          <Card title={t('migration.detail.decision_title')}>
            <p className="text-[0.8125rem] text-ink-500">{t('migration.detail.decision_help')}</p>
            <div className="mt-3.5 flex flex-wrap items-end gap-2">
              <div className="min-w-[260px] flex-1">
                <label className="label" htmlFor="reject-reason">
                  {t('migration.detail.reject_reason_label')}
                </label>
                <input
                  id="reject-reason"
                  className="field"
                  value={rejectReason}
                  onChange={(e) => setRejectReason(e.target.value)}
                  placeholder={t('migration.detail.reject_reason_placeholder')}
                />
              </div>
              <button
                className="btn-secondary"
                disabled={busy || rejectReason.trim().length < 5}
                title={
                  rejectReason.trim().length < 5
                    ? t('migration.detail.reject_reason_placeholder')
                    : undefined
                }
                onClick={() =>
                  action(async () => {
                    await migrationService.rejectRun(run.id, rejectReason.trim());
                  })
                }
              >
                <X className="h-4 w-4" /> {t('migration.detail.action.reject')}
              </button>
              <button
                className="btn-primary"
                disabled={busy}
                onClick={() => action(() => migrationService.approveRun(run.id))}
              >
                <Check className="h-4 w-4" /> {t('migration.detail.action.approve')}
              </button>
            </div>
          </Card>
        )}

        <PoolFlow
          legacy={legacyAmount}
          ironwood={ironwoodAmount}
          pct={pct}
          live={live}
          caption={t('migration.detail.batches_done', {
            done: submitted.length,
            total: run.item_count,
          })}
        />

        <Card>
          <StatRow>
            <Stat label={t('migration.detail.total')}>
              <Amount value={run.total_amount} strong />
            </Stat>
            <Stat label={t('migration.detail.progress')}>
              <span className="num">
                {submitted.length}/{run.item_count}
              </span>
            </Stat>
            <Stat label={t('migration.detail.created_by')}>
              <span className="text-ink-700">user #{run.created_by_user_id}</span>
            </Stat>
            <Stat label={t('migration.detail.approved_by')}>
              {run.approved_by_user_id ? (
                <span className="text-ink-700">user #{run.approved_by_user_id}</span>
              ) : (
                <span className="text-ink-300">—</span>
              )}
            </Stat>
          </StatRow>

          {(run.reject_reason || run.notes) && (
            <div className="mt-4 space-y-2 border-t border-line-100 pt-4">
              {run.reject_reason && (
                <p className="text-[0.8125rem] text-bad-700">
                  <span className="text-ink-400">{t('migration.detail.reject_reason')}: </span>
                  {run.reject_reason}
                </p>
              )}
              {run.notes && (
                <p className="whitespace-pre-wrap text-[0.8125rem] text-ink-500">
                  <span className="text-ink-400">{t('migration.detail.notes')}: </span>
                  {run.notes}
                </p>
              )}
            </div>
          )}
        </Card>

        <Card title={t('migration.detail.batches')} flush>
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>#</th>
                  <th className="cell-num">{t('migration.detail.col.amount')}</th>
                  <th>{t('migration.detail.col.scheduled')}</th>
                  <th>{t('migration.detail.col.status')}</th>
                  <th>{t('migration.detail.col.tx')}</th>
                  <th>{t('migration.detail.col.reason')}</th>
                </tr>
              </thead>
              <tbody>
                {items.map((item) => (
                  <tr key={item.id}>
                    <td className="num text-ink-400">{item.seq + 1}</td>
                    <td className="cell-num">
                      <Amount value={item.amount} unit={null} />
                    </td>
                    <td className="text-ink-500">
                      {item.scheduled_at ? (
                        <TimeAgo value={item.scheduled_at} />
                      ) : (
                        t('migration.detail.immediately')
                      )}
                    </td>
                    <td>
                      <ItemStatusBadge
                        status={item.status}
                        label={t(`migration.item_status.${item.status}`)}
                      />
                    </td>
                    <td>
                      <Hash value={item.tx_hash} />
                    </td>
                    <td
                      className="max-w-[240px] truncate text-[0.75rem] text-bad-600"
                      title={item.error_message || ''}
                    >
                      {item.error_message || <span className="text-ink-300">—</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      </div>
    </>
  );
}
