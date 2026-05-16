import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Plus, RefreshCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { PayrollRun } from '../../types/payroll';

/**
 * F3.1 — Payroll Run list. Shows all runs with status + counts.
 */
export function PayrollRunList() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<PayrollRun[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    try {
      const data = await payrollService.listRuns(20, 0);
      setRuns(data.runs);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, []);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-4 max-w-5xl">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('payroll.run_list.title')}</h1>
        <div className="flex gap-2">
          <button className="btn-ghost" onClick={load} aria-label={t('common.refresh')}>
            <RefreshCw className="w-4 h-4" />
          </button>
          <button className="btn-primary" onClick={() => navigate('/payroll/runs/new')}>
            <Plus className="w-4 h-4 inline mr-1" />
            {t('payroll.run_list.new_run')}
          </button>
        </div>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <table className="w-full text-sm">
          <thead className="text-xs text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('payroll.run_list.col.period')}</th>
              <th className="text-left p-2">{t('payroll.run_list.col.source')}</th>
              <th className="text-right p-2">{t('payroll.run_list.col.amount')}</th>
              <th className="text-left p-2">{t('payroll.run_list.col.status')}</th>
              <th className="text-right p-2">{t('payroll.run_list.col.items')}</th>
              <th className="text-left p-2">{t('payroll.run_list.col.created_at')}</th>
            </tr>
          </thead>
          <tbody>
            {runs.map(r => (
              <tr
                key={r.id}
                className="border-t border-gray-100 hover:bg-gray-50 cursor-pointer"
                onClick={() => navigate(`/payroll/runs/${r.id}`)}
              >
                <td className="p-2 font-mono text-xs">{r.id}</td>
                <td className="p-2">{r.pay_period || '—'}</td>
                <td className="p-2">{r.source_chain} / {r.source_token}</td>
                <td className="p-2 text-right font-mono">{r.source_amount}</td>
                <td className="p-2">
                  <span className={`text-xs px-2 py-0.5 rounded ${statusColorClass(r.status)}`}>
                    {t(`payroll.run_status.${r.status}`)}
                  </span>
                </td>
                <td className="p-2 text-right text-xs">
                  {r.succeeded_items ?? 0} / {r.total_items ?? 0}
                  {r.failed_items ? <span className="text-red-600"> ({r.failed_items} ⚠️)</span> : null}
                </td>
                <td className="p-2 text-xs">{new Date(r.created_at).toLocaleString()}</td>
              </tr>
            ))}
            {runs.length === 0 && (
              <tr><td colSpan={7} className="p-4 text-center text-gray-400">{t('payroll.run_list.empty')}</td></tr>
            )}
          </tbody>
        </table>
      </Card>
    </div>
  );
}

function statusColorClass(status: string): string {
  switch (status) {
    case 'completed':       return 'bg-green-100 text-green-800';
    case 'partial_success': return 'bg-yellow-100 text-yellow-800';
    case 'executing':       return 'bg-blue-100 text-blue-800';
    case 'awaiting_approval':
    case 'approved':        return 'bg-yellow-100 text-yellow-800';
    case 'rejected':
    case 'failed':          return 'bg-red-100 text-red-800';
    case 'draft':           return 'bg-gray-100 text-gray-800';
    default:                return 'bg-gray-100 text-gray-700';
  }
}
