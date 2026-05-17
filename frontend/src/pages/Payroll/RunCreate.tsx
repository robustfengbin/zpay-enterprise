import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Upload, AlertTriangle, CheckCircle, FileText } from 'lucide-react';
import { isAxiosError } from 'axios';
import { Card } from '../../components/Common';
import { payrollService, walletService } from '../../services/api';
import type {
  CreatePayrollRunResponse,
  CsvPreviewRow,
  PayrollItemInput,
} from '../../types/payroll';
import type { Wallet } from '../../types';

/**
 * F3.1 — Create a new Payroll Run (M1 single-wallet homogeneous batch).
 *
 * Flow:
 *  1. Pick source wallet (chain implied by wallet)
 *  2. Upload CSV — parsed client-side (no /payroll/csv/preview endpoint in M1)
 *     Columns: employee_code, employee_address, amount, memo
 *  3. POST /payroll/runs — server returns per-row validation_errors on 422.
 */
export function PayrollRunCreate() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);

  const [wallets, setWallets] = useState<Wallet[]>([]);
  const [sourceWalletId, setSourceWalletId] = useState<number | null>(null);
  const [payPeriod, setPayPeriod] = useState('');
  const [notes, setNotes] = useState('');
  const [rows, setRows] = useState<CsvPreviewRow[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [serverErrors, setServerErrors] = useState<CreatePayrollRunResponse['validation_errors']>([]);

  useEffect(() => {
    walletService.listWallets()
      .then(setWallets)
      .catch(e => setError((e as Error).message));
  }, []);

  const counts = useMemo(() => {
    const valid = rows.filter(r => r.errors.length === 0).length;
    return { total: rows.length, valid, invalid: rows.length - valid };
  }, [rows]);

  function onFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setError(null);
    setServerErrors([]);
    const reader = new FileReader();
    reader.onload = () => {
      const text = String(reader.result || '');
      setRows(parseCsv(text));
    };
    reader.onerror = () => setError(t('payroll.create.csv_read_error'));
    reader.readAsText(file);
  }

  async function onSubmit() {
    if (sourceWalletId === null) {
      setError(t('payroll.create.err_no_wallet'));
      return;
    }
    if (!payPeriod.trim()) {
      setError(t('payroll.create.err_no_period'));
      return;
    }
    if (counts.invalid > 0) {
      if (!confirm(t('payroll.create.confirm_with_invalid', { count: counts.invalid }))) return;
    }
    const items: PayrollItemInput[] = rows
      .filter(r => r.errors.length === 0)
      .map(r => ({
        employee_code: r.employee_code || undefined,
        employee_address: r.employee_address,
        amount: r.amount,
        memo: r.memo || undefined,
      }));
    if (items.length === 0) {
      setError(t('payroll.create.err_no_valid_rows'));
      return;
    }
    setSubmitting(true);
    setError(null);
    setServerErrors([]);
    try {
      const resp = await payrollService.createRun({
        pay_period: payPeriod.trim(),
        source_wallet_id: sourceWalletId,
        items,
        notes: notes.trim() || undefined,
      });
      navigate(`/payroll/runs/${resp.run_id}`);
    } catch (e) {
      if (isAxiosError(e) && e.response?.status === 422 && e.response?.data) {
        const payload = e.response.data as CreatePayrollRunResponse;
        setServerErrors(payload.validation_errors || []);
        setError(t('payroll.create.err_server_validation', { count: payload.validation_errors?.length || 0 }));
      } else {
        setError((e as Error).message);
      }
    } finally {
      setSubmitting(false);
    }
  }

  const selectedWallet = wallets.find(w => w.id === sourceWalletId);

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('payroll.create.title')}</h1>

      <Card>
        <div className="grid grid-cols-3 gap-3 text-sm">
          <label>
            <span className="text-gray-600">{t('payroll.create.pay_period')}</span>
            <input
              className="mt-1 w-full border rounded p-2"
              value={payPeriod}
              onChange={e => setPayPeriod(e.target.value)}
              placeholder="2026-06"
            />
          </label>
          <label>
            <span className="text-gray-600">{t('payroll.create.source_wallet')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={sourceWalletId ?? ''}
              onChange={e => setSourceWalletId(e.target.value ? Number(e.target.value) : null)}
            >
              <option value="">{t('payroll.create.pick_wallet')}</option>
              {wallets.map(w => (
                <option key={w.id} value={w.id}>{w.name} ({w.chain})</option>
              ))}
            </select>
          </label>
          <label>
            <span className="text-gray-600">{t('payroll.create.notes')}</span>
            <input
              className="mt-1 w-full border rounded p-2"
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder={t('payroll.create.notes_placeholder')}
            />
          </label>
        </div>
        {selectedWallet && (
          <p className="mt-2 text-xs text-gray-500 font-mono">{selectedWallet.address}</p>
        )}
      </Card>

      <Card>
        <div className="space-y-3">
          <p className="text-sm text-gray-600">{t('payroll.create.csv_help')}</p>
          <p className="text-xs text-gray-500 font-mono">
            employee_code, employee_address, amount, memo
          </p>
          <input
            ref={fileRef}
            type="file"
            accept=".csv,text/csv"
            className="hidden"
            onChange={onFileChange}
          />
          <button
            className="btn-secondary"
            onClick={() => fileRef.current?.click()}
            disabled={submitting}
          >
            <Upload className="w-4 h-4 inline mr-1" />
            {t('payroll.create.upload_csv')}
          </button>
          {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}
          {serverErrors.length > 0 && (
            <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-xs">
              <p className="font-semibold mb-1">{t('payroll.create.server_validation')}:</p>
              <ul className="list-disc list-inside space-y-0.5">
                {serverErrors.map((er, i) => (
                  <li key={i}>row {er.row_index + 1} · {er.field}: {er.message}</li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </Card>

      {rows.length > 0 && (
        <>
          <Card>
            <div className="flex items-center gap-4 text-sm">
              <span className="font-semibold">{t('payroll.create.preview_summary')}:</span>
              <span><CheckCircle className="w-4 h-4 inline text-green-600 mr-1" />{counts.valid} {t('payroll.create.valid')}</span>
              {counts.invalid > 0 && (
                <span className="text-red-600">
                  <AlertTriangle className="w-4 h-4 inline mr-1" />{counts.invalid} {t('payroll.create.invalid')}
                </span>
              )}
              <span className="text-gray-500">{t('payroll.create.total')}: {counts.total}</span>
            </div>
          </Card>

          <Card>
            <table className="w-full text-xs">
              <thead className="text-gray-500 uppercase">
                <tr>
                  <th className="text-left p-2">#</th>
                  <th className="text-left p-2">{t('payroll.create.col.code')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.address')}</th>
                  <th className="text-right p-2">{t('payroll.create.col.amount')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.memo')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.checks')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map(row => (
                  <tr key={row.row_index} className={`border-t border-gray-100 ${row.errors.length ? 'bg-red-50' : ''}`}>
                    <td className="p-2">{row.row_index + 1}</td>
                    <td className="p-2">{row.employee_code || '—'}</td>
                    <td className="p-2 font-mono truncate max-w-[200px]">{row.employee_address}</td>
                    <td className="p-2 text-right font-mono">{row.amount}</td>
                    <td className="p-2">{row.memo || '—'}</td>
                    <td className="p-2">
                      {row.errors.length === 0
                        ? <span className="text-green-700 text-xs">✓</span>
                        : <span className="text-red-700 text-xs">{row.errors.join(', ')}</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>

          <div className="flex gap-2">
            <button
              className="btn-primary"
              disabled={submitting || counts.valid === 0 || sourceWalletId === null}
              onClick={onSubmit}
            >
              <FileText className="w-4 h-4 inline mr-1" />
              {submitting ? t('common.loading') : t('payroll.create.submit')}
            </button>
            <button className="btn-ghost" onClick={() => navigate('/payroll/runs')}>
              {t('common.cancel')}
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/**
 * Minimal CSV parser — strips quotes, splits on comma, trims fields.
 * Skips empty lines + header rows that start with `employee_code`.
 */
function parseCsv(text: string): CsvPreviewRow[] {
  const out: CsvPreviewRow[] = [];
  const lines = text.split(/\r?\n/);
  let rowIdx = 0;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (/^employee_code\s*,/i.test(line)) continue;
    const cols = line.split(',').map(c => c.trim().replace(/^"|"$/g, ''));
    const [code = '', address = '', amount = '', memo = ''] = cols;
    const errors: string[] = [];
    if (!address) errors.push('missing address');
    if (!amount) errors.push('missing amount');
    else if (!/^\d+(\.\d+)?$/.test(amount) || Number(amount) <= 0) errors.push('invalid amount');
    out.push({
      row_index: rowIdx++,
      employee_code: code,
      employee_address: address,
      amount,
      memo,
      errors,
    });
  }
  return out;
}
