import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useNavigate } from 'react-router-dom';
import { Play, RefreshCw, RotateCw, XCircle } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { ExecuteRunOutcome, PayrollRunSummary } from '../../types/payroll';

/**
 * F3.1 — Payroll Run detail.
 *
 * Backend GET /payroll/runs/{id} returns `{run, items}` (nested).
 * Execute responds with a tagged union — awaiting_approval routes the user
 * to the pending-approval queue; executed shows submitted/failed counts.
 */
export function PayrollRunDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [summary, setSummary] = useState<PayrollRunSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<ExecuteRunOutcome | null>(null);

  async function load() {
    setLoading(true);
    try {
      setSummary(await payrollService.getRun(Number(id)));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, [id]);

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
      const res = await payrollService.executeRun(Number(id));
      setOutcome(res);
      if (res.result === 'awaiting_approval') {
        // Brief flash before routing, so the user sees the pivot.
        setTimeout(() => navigate('/approval/pending'), 1200);
      }
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
  const canExecute = run.status === 'pending';
  const canCancel = run.status === 'pending' || run.status === 'awaiting_approval';
  const failedCount = items.filter(it => it.status === 'failed' || it.status === 'compensation_pending').length;
  const canRetry = failedCount > 0 && run.status !== 'executing';

  return (
    <div className="p-6 max-w-6xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('payroll.detail.title')} #{run.id}</h1>
          <p className="text-sm text-gray-500">
            {run.pay_period} · {t('payroll.detail.source_wallet')} #{run.source_wallet_id}
          </p>
        </div>
        <button className="btn-ghost" onClick={load} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      <Card>
        <dl className="grid grid-cols-4 gap-y-2 text-sm">
          <dt className="text-gray-500">{t('payroll.detail.total_amount')}</dt>
          <dd className="font-mono">{run.total_amount}</dd>
          <dt className="text-gray-500">{t('payroll.detail.status')}</dt>
          <dd>
            <span className="px-2 py-0.5 rounded bg-gray-100 text-gray-800 text-xs">
              {t(`payroll.run_status.${run.status}`)}
            </span>
          </dd>
          <dt className="text-gray-500">{t('payroll.detail.item_count')}</dt>
          <dd>{run.item_count}</dd>
          <dt className="text-gray-500">{t('payroll.detail.created_by')}</dt>
          <dd>user#{run.created_by_user_id}</dd>
          <dt className="text-gray-500">{t('payroll.detail.created_at')}</dt>
          <dd>{new Date(run.created_at).toLocaleString()}</dd>
          {run.executed_at && (
            <>
              <dt className="text-gray-500">{t('payroll.detail.executed_at')}</dt>
              <dd>{new Date(run.executed_at).toLocaleString()}</dd>
            </>
          )}
          {run.executed_by_user_id && (
            <>
              <dt className="text-gray-500">{t('payroll.detail.executed_by')}</dt>
              <dd>user#{run.executed_by_user_id}</dd>
            </>
          )}
          {run.notes && (
            <>
              <dt className="text-gray-500">{t('payroll.detail.notes')}</dt>
              <dd className="col-span-3 whitespace-pre-wrap text-gray-700">{run.notes}</dd>
            </>
          )}
        </dl>
      </Card>

      {outcome && (
        <Card>
          <h2 className="font-semibold text-sm mb-1">{t('payroll.detail.execute_outcome')}</h2>
          {outcome.result === 'awaiting_approval' ? (
            <p className="text-sm text-yellow-800">
              {t('payroll.detail.outcome.awaiting', {
                threshold: outcome.threshold,
                policy_id: outcome.policy_id,
              })}
            </p>
          ) : (
            <p className="text-sm">
              {t('payroll.detail.outcome.executed', {
                submitted: outcome.submitted,
                failed: outcome.failed,
                status: outcome.final_status,
              })}
            </p>
          )}
        </Card>
      )}

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <div className="flex gap-2">
        {canExecute && (
          <button className="btn-primary" disabled={busy} onClick={onExecute}>
            <Play className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.execute')}
          </button>
        )}
        {canRetry && (
          <button
            className="btn-secondary"
            disabled={busy}
            onClick={() => action(async () => {
              const failed = items.filter(it => it.status === 'failed' || it.status === 'compensation_pending');
              await Promise.all(failed.map(it => payrollService.retryItem(run.id, it.id)));
            })}
          >
            <RotateCw className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.retry_failed', { count: failedCount })}
          </button>
        )}
        {canCancel && (
          <button
            className="btn-ghost text-red-600"
            disabled={busy}
            onClick={() => {
              if (!confirm(t('payroll.detail.confirm_cancel'))) return;
              void action(() => payrollService.cancelRun(run.id));
            }}
          >
            <XCircle className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.cancel')}
          </button>
        )}
        {run.status === 'awaiting_approval' && (
          <button className="btn-ghost" onClick={() => navigate('/approval/pending')}>
            {t('payroll.detail.action.view_approval')}
          </button>
        )}
      </div>

      <Card>
        <h2 className="font-semibold mb-2 text-sm">{t('payroll.detail.items')}</h2>
        <table className="w-full text-xs">
          <thead className="text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('payroll.detail.col.employee')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.address')}</th>
              <th className="text-right p-2">{t('payroll.detail.col.amount')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.status')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.tx')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.reason')}</th>
            </tr>
          </thead>
          <tbody>
            {items.map(item => (
              <tr key={item.id} className="border-t border-gray-100">
                <td className="p-2 font-mono">{item.id}</td>
                <td className="p-2">{item.employee_id ? `#${item.employee_id}` : '—'}</td>
                <td className="p-2 font-mono truncate max-w-[180px]" title={item.employee_address}>
                  {item.employee_address}
                </td>
                <td className="p-2 text-right font-mono">{item.amount}</td>
                <td className="p-2">
                  <span className={`text-xs px-1.5 py-0.5 rounded ${itemStatusColor(item.status)}`}>
                    {item.status}
                  </span>
                </td>
                <td className="p-2 font-mono truncate max-w-[160px]" title={item.tx_hash || ''}>
                  {item.tx_hash || '—'}
                </td>
                <td className="p-2 text-red-700 text-xs truncate max-w-[200px]" title={item.error_message || ''}>
                  {item.error_message || '—'}
                </td>
              </tr>
            ))}
            {items.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('payroll.detail.empty_items')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function itemStatusColor(s: string): string {
  switch (s) {
    case 'confirmed':              return 'bg-green-100 text-green-800';
    case 'building':
    case 'submitted':              return 'bg-blue-100 text-blue-800';
    case 'failed':
    case 'compensation_pending':   return 'bg-red-100 text-red-800';
    case 'pending':                return 'bg-gray-100 text-gray-700';
    default:                       return 'bg-gray-100 text-gray-700';
  }
}
