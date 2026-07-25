import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import { Check, Play, RefreshCw, RotateCw, Shield, X, XCircle } from 'lucide-react';
import {
  Amount,
  Card,
  Hash,
  ItemStatusBadge,
  LoadingSpinner,
  PageHeader,
  RunStatusBadge,
  Stat,
  StatRow,
  TimeAgo,
} from '../../components/Common';
import { batchTransferService } from '../../services/api/batch-transfer';
import type {
  BatchTransferRunSummary,
  ExecuteBatchTransferOutcome,
} from '../../types/batch-transfer';

/** Poll cadence while a run is live (executor ticks every 30 s). */
const POLL_MS = 10_000;

/**
 * F4.2 — Batch transfer run detail / progress page. Same polling-first
 * shape as the migration detail page; approval, cancel and per-item retry
 * share the F4.1 semantics (approve covers the window, cancel is the only
 * stop, submitted items are on-chain and stay).
 */
export function BatchTransferRunDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const [summary, setSummary] = useState<BatchTransferRunSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExecuteBatchTransferOutcome | null>(null);
  const [rejectReason, setRejectReason] = useState('');
  const timer = useRef<ReturnType<typeof setInterval> | null>(null);

  const load = useCallback(async () => {
    try {
      setSummary(await batchTransferService.getRun(Number(id)));
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

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
      const res = await batchTransferService.executeRun(Number(id));
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
  const sent = submitted.reduce((acc, it) => acc + (Number(it.amount) || 0), 0);
  const pct = items.length > 0 ? Math.round((submitted.length / items.length) * 100) : 0;

  return (
    <>
      <PageHeader
        backTo={{ to: '/batch-transfers', label: t('batch.detail.back_to_list') }}
        title={run.title}
        subtitle={`#${run.id} · ${t('batch.detail.wallet')} #${run.source_wallet_id}`}
        meta={
          <>
            <RunStatusBadge status={run.status} label={t(`batch.run_status.${run.status}`)} />
            <span
              className={`badge ${
                run.privacy_mode === 'staggered' ? 'badge-brand' : 'badge-neutral'
              }`}
            >
              {run.privacy_mode === 'staggered' && <Shield className="h-3 w-3" />}
              {t(`batch.create.mode_${run.privacy_mode}`)}
            </span>
            {run.privacy_mode === 'staggered' && (
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
                      await batchTransferService.retryItem(run.id, it.id);
                    }
                  })
                }
              >
                <RotateCw className="h-4 w-4" />
                {t('batch.detail.action.retry_failed', { count: failed.length })}
              </button>
            )}
            {canCancel && (
              <button
                className="btn-secondary text-bad-600"
                disabled={busy}
                onClick={() => {
                  if (!confirm(t('batch.detail.confirm_cancel'))) return;
                  void action(() => batchTransferService.cancelRun(run.id));
                }}
              >
                <XCircle className="h-4 w-4" /> {t('batch.detail.action.cancel')}
              </button>
            )}
            {canExecute && (
              <button className="btn-primary" disabled={busy} onClick={onExecute}>
                <Play className="h-4 w-4" />
                {run.privacy_mode === 'staggered'
                  ? t('batch.detail.action.arm_schedule')
                  : t('batch.detail.action.execute')}
              </button>
            )}
          </>
        }
      />

      <div className="space-y-4">
        {error && <div className="alert alert-bad">{error}</div>}

        {outcome && outcome.result === 'awaiting_approval' && (
          <div className="alert alert-warn">
            {t('batch.detail.outcome_awaiting', {
              threshold: outcome.threshold,
              policy_id: outcome.policy_id,
            })}
          </div>
        )}

        {canDecide && (
          <Card title={t('batch.detail.decision_title')}>
            <p className="text-[0.8125rem] text-ink-500">{t('batch.detail.decision_help')}</p>
            <div className="mt-3.5 flex flex-wrap items-end gap-2">
              <div className="min-w-[260px] flex-1">
                <label className="label" htmlFor="reject-reason">
                  {t('batch.detail.reject_reason_label')}
                </label>
                <input
                  id="reject-reason"
                  className="field"
                  value={rejectReason}
                  onChange={(e) => setRejectReason(e.target.value)}
                  placeholder={t('batch.detail.reject_reason_placeholder')}
                />
              </div>
              <button
                className="btn-secondary"
                disabled={busy || rejectReason.trim().length < 5}
                onClick={() =>
                  action(() => batchTransferService.rejectRun(run.id, rejectReason.trim()))
                }
              >
                <X className="h-4 w-4" /> {t('batch.detail.action.reject')}
              </button>
              <button
                className="btn-primary"
                disabled={busy}
                onClick={() => action(() => batchTransferService.approveRun(run.id))}
              >
                <Check className="h-4 w-4" /> {t('batch.detail.action.approve')}
              </button>
            </div>
          </Card>
        )}

        <Card>
          <StatRow>
            <Stat label={t('batch.detail.total')}>
              <Amount value={run.total_amount} strong />
            </Stat>
            <Stat
              label={t('batch.detail.progress')}
              hint={t('batch.detail.sent_amount', { amount: sent.toFixed(8).replace(/\.?0+$/, '') })}
            >
              <span className="num">
                {submitted.length}/{run.item_count}
              </span>
            </Stat>
            <Stat label={t('batch.detail.created_by')}>
              <span className="text-ink-700">user #{run.created_by_user_id}</span>
            </Stat>
            <Stat label={t('batch.detail.approved_by')}>
              {run.approved_by_user_id ? (
                <span className="text-ink-700">user #{run.approved_by_user_id}</span>
              ) : (
                <span className="text-ink-300">—</span>
              )}
            </Stat>
          </StatRow>

          <div className="mt-4">
            <div className="progress">
              <span style={{ width: `${pct}%` }} />
            </div>
            <div className="mt-2 flex items-center justify-between text-[0.6875rem] text-ink-400">
              <span>
                {t('batch.detail.recipients_paid', {
                  done: submitted.length,
                  total: run.item_count,
                })}
              </span>
              <span className="num font-medium text-ink-700">{pct}%</span>
            </div>
          </div>

          {(run.reject_reason || run.notes) && (
            <div className="mt-4 space-y-2 border-t border-line-100 pt-4">
              {run.reject_reason && (
                <p className="text-[0.8125rem] text-bad-700">
                  <span className="text-ink-400">{t('batch.detail.reject_reason')}: </span>
                  {run.reject_reason}
                </p>
              )}
              {run.notes && (
                <p className="whitespace-pre-wrap text-[0.8125rem] text-ink-500">
                  <span className="text-ink-400">{t('batch.detail.notes')}: </span>
                  {run.notes}
                </p>
              )}
            </div>
          )}
        </Card>

        <Card title={t('batch.detail.items')} flush>
          <div className="overflow-x-auto">
            <table className="table">
              <thead>
                <tr>
                  <th>#</th>
                  <th>{t('batch.detail.col.recipient')}</th>
                  <th className="cell-num">{t('batch.detail.col.amount')}</th>
                  <th>{t('batch.detail.col.memo')}</th>
                  <th>{t('batch.detail.col.scheduled')}</th>
                  <th>{t('batch.detail.col.status')}</th>
                  <th>{t('batch.detail.col.tx')}</th>
                  <th>{t('batch.detail.col.reason')}</th>
                </tr>
              </thead>
              <tbody>
                {items.map((item) => (
                  <tr key={item.id}>
                    <td className="num text-ink-400">{item.seq + 1}</td>
                    <td>
                      <Hash value={item.recipient_address} head={12} tail={6} />
                    </td>
                    <td className="cell-num">
                      <Amount value={item.amount} unit={null} />
                    </td>
                    <td
                      className="max-w-[120px] truncate text-ink-500"
                      title={item.memo || ''}
                    >
                      {item.memo || <span className="text-ink-300">—</span>}
                    </td>
                    <td className="text-ink-500">
                      {item.scheduled_at ? (
                        <TimeAgo value={item.scheduled_at} />
                      ) : (
                        t('batch.detail.immediately')
                      )}
                    </td>
                    <td>
                      <ItemStatusBadge
                        status={item.status}
                        label={t(`batch.item_status.${item.status}`)}
                      />
                    </td>
                    <td>
                      <Hash value={item.tx_hash} />
                    </td>
                    <td
                      className="max-w-[200px] truncate text-[0.75rem] text-bad-600"
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
