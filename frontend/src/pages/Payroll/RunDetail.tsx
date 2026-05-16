import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useNavigate } from 'react-router-dom';
import { Play, RefreshCw, Clock, RotateCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { PayrollItem, PayrollRun } from '../../types/payroll';

type RunDetail = PayrollRun & { items: PayrollItem[] };

/**
 * F3.1 — Payroll Run detail: shows all items + status + actions
 *   - quote (estimate fee + proof time)
 *   - request approval (creates F2.1 run-level approval if amount exceeds policy)
 *   - execute (after approved or low-amount)
 *   - retry failed items
 */
export function PayrollRunDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [run, setRun] = useState<RunDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quote, setQuote] = useState<{ estimated_fee: string; estimated_proof_seconds: number } | null>(null);

  async function load() {
    setLoading(true);
    try {
      setRun(await payrollService.getRun(Number(id)));
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

  if (loading) return <LoadingSpinner />;
  if (!run) return <div className="p-6 text-red-600">{error || t('common.not_found')}</div>;

  const canQuote = run.status === 'draft';
  const canExecute = run.status === 'approved' || (run.status === 'draft' && run.items.length > 0); // assuming low-amount = no approval needed
  const canRetry = run.status === 'partial_success' || (run.failed_items ?? 0) > 0;

  return (
    <div className="p-6 max-w-6xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold">{t('payroll.detail.title')} #{run.id}</h1>
          <p className="text-sm text-gray-500">{run.pay_period || '—'} · {run.source_chain} / {run.source_token}</p>
        </div>
        <button className="btn-ghost" onClick={load} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      <Card>
        <dl className="grid grid-cols-4 gap-y-2 text-sm">
          <dt className="text-gray-500">{t('payroll.detail.source_amount')}</dt>
          <dd className="font-mono">{run.source_amount} {run.source_token}</dd>
          <dt className="text-gray-500">{t('payroll.detail.status')}</dt>
          <dd>{t(`payroll.run_status.${run.status}`)}</dd>
          <dt className="text-gray-500">{t('payroll.detail.items_total')}</dt>
          <dd>{run.items.length}</dd>
          <dt className="text-gray-500">{t('payroll.detail.created_by')}</dt>
          <dd>user#{run.created_by}</dd>
          <dt className="text-gray-500">{t('payroll.detail.created_at')}</dt>
          <dd>{new Date(run.created_at).toLocaleString()}</dd>
          {run.approved_by && (
            <>
              <dt className="text-gray-500">{t('payroll.detail.approved_by')}</dt>
              <dd>user#{run.approved_by}</dd>
            </>
          )}
        </dl>
      </Card>

      {quote && (
        <Card>
          <h2 className="font-semibold text-sm mb-2 flex items-center gap-1">
            <Clock className="w-4 h-4" /> {t('payroll.detail.quote')}
          </h2>
          <p className="text-sm">
            {t('payroll.detail.estimated_fee')}: <span className="font-mono">{quote.estimated_fee}</span>
            <span className="ml-4">{t('payroll.detail.estimated_proof_time')}: ~{quote.estimated_proof_seconds}s</span>
          </p>
        </Card>
      )}

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <div className="flex gap-2">
        {canQuote && (
          <button
            className="btn-secondary"
            disabled={busy}
            onClick={() => action(async () => setQuote(await payrollService.quoteRun(run.id)))}
          >
            <Clock className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.quote')}
          </button>
        )}
        {canExecute && (
          <button
            className="btn-primary"
            disabled={busy}
            onClick={() => action(() => payrollService.executeRun(run.id))}
          >
            <Play className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.execute')}
          </button>
        )}
        {canRetry && (
          <button
            className="btn-secondary"
            disabled={busy}
            onClick={() => action(async () => {
              // Backend exposes per-item retry only — fan out client-side.
              const failed = run.items.filter(it => it.status === 'failed' || it.status === 'compensation_pending');
              await Promise.all(failed.map(it => payrollService.retryItem(run.id, it.id)));
            })}
          >
            <RotateCw className="w-4 h-4 inline mr-1" /> {t('payroll.detail.action.retry_failed')}
          </button>
        )}
        {run.status === 'awaiting_approval' && (
          <button
            className="btn-ghost"
            onClick={() => navigate(`/approval/${run.id}`)}
          >
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
              <th className="text-left p-2">{t('payroll.detail.col.chain')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.token')}</th>
              <th className="text-right p-2">{t('payroll.detail.col.amount')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.privacy')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.status')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.tx')}</th>
              <th className="text-left p-2">{t('payroll.detail.col.reason')}</th>
            </tr>
          </thead>
          <tbody>
            {run.items.map(item => (
              <tr key={item.id} className="border-t border-gray-100">
                <td className="p-2 font-mono">{item.id}</td>
                <td className="p-2">{item.employee_name}</td>
                <td className="p-2">{item.target_chain}</td>
                <td className="p-2">{item.target_token}</td>
                <td className="p-2 text-right font-mono">{item.amount_target || item.amount_source || '—'}</td>
                <td className="p-2">{item.privacy_mode}</td>
                <td className="p-2">
                  <span className={`text-xs px-1.5 py-0.5 rounded ${itemStatusColor(item.status)}`}>
                    {item.status}
                  </span>
                </td>
                <td className="p-2 font-mono truncate max-w-[120px]">
                  {item.linked_transfer_ids.length > 0 ? `#${item.linked_transfer_ids.join(',')}` : '—'}
                </td>
                <td className="p-2 text-red-700 text-xs truncate max-w-[160px]">{item.failure_reason || '—'}</td>
              </tr>
            ))}
            {run.items.length === 0 && (
              <tr><td colSpan={9} className="p-4 text-center text-gray-400">{t('payroll.detail.empty_items')}</td></tr>
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
