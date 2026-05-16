import React, { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Upload, AlertTriangle, CheckCircle, FileText } from 'lucide-react';
import { Card } from '../../components/Common';
import { payrollService } from '../../services/api';
import type { CsvPreviewResponse, CsvPreviewRow, PayrollPrivacyMode } from '../../types/payroll';

/**
 * F3.1 — Create a new Payroll Run.
 * Step 1: upload CSV → server-side preview with per-row validation
 *        (address syntax / chain match / address_book hit)
 * Step 2: review preview, edit invalid rows inline (M2 enhancement),
 *         confirm to commit run as 'draft' status
 * Step 3: navigate to detail view to trigger quote → approval → execute
 */
export function PayrollRunCreate() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);

  const [payPeriod, setPayPeriod] = useState('');
  const [sourceChain, setSourceChain] = useState('zcash');
  const [sourceToken, setSourceToken] = useState('ZEC');
  const [preview, setPreview] = useState<CsvPreviewResponse | null>(null);
  const [uploading, setUploading] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function onFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setUploading(true);
    setError(null);
    try {
      const data = await payrollService.previewCsv(file);
      setPreview(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setUploading(false);
    }
  }

  async function onSubmit() {
    if (!preview) return;
    if (preview.invalid_rows > 0) {
      if (!confirm(t('payroll.create.confirm_with_invalid', { count: preview.invalid_rows }))) return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const validRows = preview.rows.filter(r => r.errors.length === 0);
      const sourceAmount = validRows.reduce((acc, r) => acc + parseFloat(r.amount), 0).toString();
      const run = await payrollService.createRun({
        pay_period: payPeriod || undefined,
        source_chain: sourceChain,
        source_token: sourceToken,
        source_amount: sourceAmount,
        items: validRows.map(r => ({
          employee_name: r.employee_name,
          wallet_address: r.wallet_address,
          target_chain: r.target_chain,
          target_token: r.target_token,
          amount_source: r.amount,
          privacy_mode: r.privacy_mode,
        })),
      });
      navigate(`/payroll/runs/${run.id}`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

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
            <span className="text-gray-600">{t('payroll.create.source_chain')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={sourceChain}
              onChange={e => setSourceChain(e.target.value)}
            >
              <option value="zcash">zcash</option>
              <option value="ethereum">ethereum</option>
            </select>
          </label>
          <label>
            <span className="text-gray-600">{t('payroll.create.source_token')}</span>
            <input
              className="mt-1 w-full border rounded p-2"
              value={sourceToken}
              onChange={e => setSourceToken(e.target.value)}
            />
          </label>
        </div>
      </Card>

      <Card>
        <div className="space-y-3">
          <p className="text-sm text-gray-600">{t('payroll.create.csv_help')}</p>
          <p className="text-xs text-gray-500 font-mono">
            employee_name, wallet_address, target_chain, target_token, amount, privacy_mode
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
            disabled={uploading}
          >
            <Upload className="w-4 h-4 inline mr-1" />
            {uploading ? t('common.loading') : t('payroll.create.upload_csv')}
          </button>
          {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}
        </div>
      </Card>

      {preview && (
        <>
          <Card>
            <div className="flex items-center gap-4 text-sm">
              <span className="font-semibold">{t('payroll.create.preview_summary')}:</span>
              <span><CheckCircle className="w-4 h-4 inline text-green-600 mr-1" />{preview.valid_rows} {t('payroll.create.valid')}</span>
              {preview.invalid_rows > 0 && (
                <span className="text-red-600">
                  <AlertTriangle className="w-4 h-4 inline mr-1" />{preview.invalid_rows} {t('payroll.create.invalid')}
                </span>
              )}
              <span className="text-gray-500">{t('payroll.create.total')}: {preview.total_rows}</span>
            </div>
          </Card>

          <Card>
            <table className="w-full text-xs">
              <thead className="text-gray-500 uppercase">
                <tr>
                  <th className="text-left p-2">#</th>
                  <th className="text-left p-2">{t('payroll.create.col.employee')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.address')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.chain')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.token')}</th>
                  <th className="text-right p-2">{t('payroll.create.col.amount')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.privacy')}</th>
                  <th className="text-left p-2">{t('payroll.create.col.checks')}</th>
                </tr>
              </thead>
              <tbody>
                {preview.rows.map(row => (
                  <PayrollPreviewRow key={row.row_index} row={row} />
                ))}
              </tbody>
            </table>
          </Card>

          <div className="flex gap-2">
            <button
              className="btn-primary"
              disabled={submitting || preview.valid_rows === 0}
              onClick={onSubmit}
            >
              <FileText className="w-4 h-4 inline mr-1" />
              {t('payroll.create.create_draft')}
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

function PayrollPreviewRow({ row }: { row: CsvPreviewRow }) {
  const { t } = useTranslation();
  const hasError = row.errors.length > 0;
  return (
    <tr className={`border-t border-gray-100 ${hasError ? 'bg-red-50' : ''}`}>
      <td className="p-2">{row.row_index}</td>
      <td className="p-2">{row.employee_name}</td>
      <td className="p-2 font-mono truncate max-w-[180px]">{row.wallet_address}</td>
      <td className="p-2">{row.target_chain}</td>
      <td className="p-2">{row.target_token}</td>
      <td className="p-2 text-right font-mono">{row.amount}</td>
      <td className="p-2">{row.privacy_mode}</td>
      <td className="p-2">
        {hasError ? (
          <span className="text-red-700 text-xs">{row.errors.join(', ')}</span>
        ) : (
          <div className="flex gap-1 text-xs">
            <span className="text-green-700">✓</span>
            {row.in_address_book && <span title={t('payroll.create.in_address_book')}>📒</span>}
            {!row.address_chain_match && <span className="text-yellow-600" title={t('payroll.create.chain_mismatch_warn')}>⚠️</span>}
          </div>
        )}
      </td>
    </tr>
  );
}
