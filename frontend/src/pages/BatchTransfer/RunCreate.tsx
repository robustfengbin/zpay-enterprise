import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  FileSpreadsheet,
  Send,
  Shield,
  Upload,
  X,
} from 'lucide-react';
import { Amount, Card, Hash, PageHeader } from '../../components/Common';
import { batchTransferService } from '../../services/api/batch-transfer';
import { walletService } from '../../services/api';
import type { BatchCsvRow, BatchValidationError, PrivacyMode } from '../../types/batch-transfer';
import type { Wallet } from '../../types';

const CSV_TEMPLATE =
  'recipient_address,amount,memo\nu1exampleaddress...,0.5,invoice #1024\nu1anotheraddress...,1.25,\n';

/**
 * F4.2 — Create a batch privacy transfer run (PRD-F4 §5).
 *
 * Flow:
 *  1. Title + source Zcash wallet
 *  2. Privacy scheduling: off (all at once) or staggered (batches over a
 *     window, optional per-transfer cap that splits big rows)
 *  3. Upload CSV — parsed client-side. Columns: recipient_address, amount, memo
 *  4. POST /batch-transfers — server returns per-row validation_errors,
 *     shown inline against the preview table.
 */
export function BatchTransferRunCreate() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const fileRef = useRef<HTMLInputElement>(null);

  const [wallets, setWallets] = useState<Wallet[]>([]);
  const [sourceWalletId, setSourceWalletId] = useState<number | null>(null);
  const [title, setTitle] = useState('');
  const [notes, setNotes] = useState('');
  const [privacyMode, setPrivacyMode] = useState<PrivacyMode>('staggered');
  const [batchCount, setBatchCount] = useState('4');
  const [windowHours, setWindowHours] = useState('24');
  const [maxPerTransfer, setMaxPerTransfer] = useState('');
  const [rows, setRows] = useState<BatchCsvRow[]>([]);
  const [fileName, setFileName] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [serverErrors, setServerErrors] = useState<BatchValidationError[]>([]);

  useEffect(() => {
    walletService
      .listWallets()
      .then((ws) => setWallets(ws.filter((w) => w.chain === 'zcash')))
      .catch((e) => setError((e as Error).message));
  }, []);

  const counts = useMemo(() => {
    const ok = rows.filter((r) => r.errors.length === 0);
    return {
      rows: rows.length,
      valid: ok.length,
      invalid: rows.length - ok.length,
      total: ok.reduce((acc, r) => acc + Number(r.amount || 0), 0),
    };
  }, [rows]);

  // Server errors are keyed by the submitted (valid-rows-only) index; map
  // them back onto the preview rows so they highlight the right line.
  const serverErrorsByRow = useMemo(() => {
    const validRows = rows.filter((r) => r.errors.length === 0);
    const map = new Map<number, BatchValidationError[]>();
    for (const er of serverErrors) {
      const row = validRows[er.row_index];
      if (!row) continue;
      const list = map.get(row.row_index) ?? [];
      list.push(er);
      map.set(row.row_index, list);
    }
    return map;
  }, [rows, serverErrors]);

  function readFile(file: File) {
    setError(null);
    setServerErrors([]);
    setFileName(file.name);
    const reader = new FileReader();
    reader.onload = () => setRows(parseCsv(String(reader.result || '')));
    reader.onerror = () => setError(t('batch.create.csv_read_error'));
    reader.readAsText(file);
  }

  function onFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (file) readFile(file);
  }

  function onDrop(e: React.DragEvent) {
    e.preventDefault();
    setDragging(false);
    const file = e.dataTransfer.files?.[0];
    if (file) readFile(file);
  }

  function downloadTemplate() {
    const url = URL.createObjectURL(new Blob([CSV_TEMPLATE], { type: 'text/csv' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = 'zpay-batch-transfer-template.csv';
    a.click();
    URL.revokeObjectURL(url);
  }

  async function onSubmit() {
    if (!title.trim()) {
      setError(t('batch.create.err_no_title'));
      return;
    }
    if (sourceWalletId === null) {
      setError(t('batch.create.err_no_wallet'));
      return;
    }
    const items = rows
      .filter((r) => r.errors.length === 0)
      .map((r) => ({
        recipient_address: r.recipient_address,
        amount: r.amount,
        memo: r.memo || undefined,
      }));
    if (items.length === 0) {
      setError(t('batch.create.err_no_valid_rows'));
      return;
    }
    if (counts.invalid > 0 && !confirm(t('batch.create.confirm_with_invalid', { count: counts.invalid })))
      return;

    setSubmitting(true);
    setError(null);
    setServerErrors([]);
    try {
      const resp = await batchTransferService.createRun({
        title: title.trim(),
        source_wallet_id: sourceWalletId,
        privacy_mode: privacyMode,
        batch_count: privacyMode === 'staggered' ? Number(batchCount) || undefined : undefined,
        window_hours: privacyMode === 'staggered' ? Number(windowHours) || undefined : undefined,
        max_per_transfer:
          privacyMode === 'staggered' && maxPerTransfer.trim() ? maxPerTransfer.trim() : undefined,
        items,
        notes: notes.trim() || undefined,
      });
      if (resp.validation_errors.length > 0) {
        setServerErrors(resp.validation_errors);
        setError(t('batch.create.err_server_validation', { count: resp.validation_errors.length }));
        return;
      }
      navigate(`/batch-transfers/${resp.run_id}`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  const selectedWallet = wallets.find((w) => w.id === sourceWalletId);

  return (
    <>
      <PageHeader
        backTo={{ to: '/batch-transfers', label: t('batch.detail.back_to_list') }}
        title={t('batch.create.title')}
        subtitle={t('batch.create.subtitle')}
      />

      <div className="space-y-4">
        <Card title={t('batch.create.section_details')}>
          <div className="grid gap-4 sm:grid-cols-3">
            <div>
              <label className="label" htmlFor="run-title">
                {t('batch.create.run_title')}
              </label>
              <input
                id="run-title"
                className="field"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder={t('batch.create.run_title_placeholder')}
                maxLength={120}
              />
            </div>
            <div>
              <label className="label" htmlFor="source-wallet">
                {t('batch.create.source_wallet')}
              </label>
              <select
                id="source-wallet"
                className="field"
                value={sourceWalletId ?? ''}
                onChange={(e) => setSourceWalletId(e.target.value ? Number(e.target.value) : null)}
              >
                <option value="">{t('batch.create.pick_wallet')}</option>
                {wallets.map((w) => (
                  <option key={w.id} value={w.id}>
                    #{w.id} {w.name}
                  </option>
                ))}
              </select>
              {selectedWallet && (
                <div className="mt-1.5">
                  <Hash value={selectedWallet.address} head={14} tail={8} />
                </div>
              )}
            </div>
            <div>
              <label className="label" htmlFor="run-notes">
                {t('batch.create.notes')}
              </label>
              <input
                id="run-notes"
                className="field"
                value={notes}
                onChange={(e) => setNotes(e.target.value)}
                placeholder={t('batch.create.notes_placeholder')}
              />
            </div>
          </div>
        </Card>

        <Card
          title={
            <h3 className="card-title flex items-center gap-1.5">
              <Shield className="h-3.5 w-3.5 text-brand-600" />
              {t('batch.create.privacy_title')}
            </h3>
          }
        >
          <div className="grid gap-3 sm:grid-cols-2">
            <ModeOption
              selected={privacyMode === 'staggered'}
              onSelect={() => setPrivacyMode('staggered')}
              title={t('batch.create.mode_staggered')}
              help={t('batch.create.mode_staggered_help')}
            />
            <ModeOption
              selected={privacyMode === 'off'}
              onSelect={() => setPrivacyMode('off')}
              title={t('batch.create.mode_off')}
              help={t('batch.create.mode_off_help')}
            />
          </div>

          {privacyMode === 'staggered' && (
            <div className="mt-4 grid gap-4 sm:grid-cols-3">
              <div>
                <label className="label" htmlFor="batch-count">
                  {t('batch.create.batch_count')}
                </label>
                <input
                  id="batch-count"
                  type="number"
                  min={1}
                  max={50}
                  className="field num"
                  value={batchCount}
                  onChange={(e) => setBatchCount(e.target.value)}
                />
              </div>
              <div>
                <label className="label" htmlFor="window-hours">
                  {t('batch.create.window_hours')}
                </label>
                <input
                  id="window-hours"
                  type="number"
                  min={1}
                  max={336}
                  className="field num"
                  value={windowHours}
                  onChange={(e) => setWindowHours(e.target.value)}
                />
              </div>
              <div>
                <label className="label" htmlFor="max-per-transfer">
                  {t('batch.create.max_per_transfer')}
                </label>
                <input
                  id="max-per-transfer"
                  className="field num"
                  value={maxPerTransfer}
                  onChange={(e) => setMaxPerTransfer(e.target.value)}
                  placeholder={t('batch.create.max_per_transfer_placeholder')}
                />
                <p className="hint">{t('batch.create.max_per_transfer_help')}</p>
              </div>
            </div>
          )}
        </Card>

        <Card
          title={t('batch.create.section_recipients')}
          actions={
            <button className="btn-ghost btn-sm" onClick={downloadTemplate}>
              <Download className="h-3.5 w-3.5" /> {t('batch.create.download_template')}
            </button>
          }
        >
          <input
            ref={fileRef}
            type="file"
            accept=".csv,text/csv"
            className="hidden"
            onChange={onFileChange}
          />

          {rows.length === 0 ? (
            <div
              onDragOver={(e) => {
                e.preventDefault();
                setDragging(true);
              }}
              onDragLeave={() => setDragging(false)}
              onDrop={onDrop}
              onClick={() => fileRef.current?.click()}
              className={`flex cursor-pointer flex-col items-center justify-center rounded-[10px] border border-dashed px-6 py-10 text-center transition-colors ${
                dragging
                  ? 'border-brand-400 bg-brand-50'
                  : 'border-line-300 bg-surface-2 hover:border-brand-300 hover:bg-brand-50/40'
              }`}
            >
              <FileSpreadsheet className="h-6 w-6 text-ink-300" />
              <p className="mt-2.5 text-[0.8125rem] font-medium text-ink-900">
                {t('batch.create.drop_csv')}
              </p>
              <p className="mt-1 text-[0.75rem] text-ink-400">{t('batch.create.csv_help')}</p>
              <code className="mono mt-2.5 rounded-md border border-line-200 bg-surface px-2 py-1 text-[0.6875rem] text-ink-500">
                recipient_address, amount, memo
              </code>
            </div>
          ) : (
            <>
              <div className="flex flex-wrap items-center justify-between gap-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="badge badge-neutral">
                    <FileSpreadsheet className="h-3 w-3" />
                    {fileName}
                  </span>
                  <span className="badge badge-ok">
                    <CheckCircle2 className="h-3 w-3" />
                    {counts.valid} {t('batch.create.valid')}
                  </span>
                  {counts.invalid > 0 && (
                    <span className="badge badge-bad">
                      <AlertTriangle className="h-3 w-3" />
                      {counts.invalid} {t('batch.create.invalid')}
                    </span>
                  )}
                  <span className="text-[0.75rem] text-ink-400">
                    {t('batch.create.total_amount')}: <Amount value={counts.total} />
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <button className="btn-secondary btn-sm" onClick={() => fileRef.current?.click()}>
                    <Upload className="h-3.5 w-3.5" /> {t('batch.create.upload_csv')}
                  </button>
                  <button
                    className="btn-ghost btn-icon btn-sm"
                    title={t('common.cancel')}
                    onClick={() => {
                      setRows([]);
                      setFileName(null);
                      setServerErrors([]);
                    }}
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </div>
              </div>

              <div className="mt-3.5 max-h-[420px] overflow-auto rounded-[10px] border border-line-200">
                <table className="table">
                  <thead>
                    <tr>
                      <th>#</th>
                      <th>{t('batch.create.col.address')}</th>
                      <th className="cell-num">{t('batch.create.col.amount')}</th>
                      <th>{t('batch.create.col.memo')}</th>
                      <th>{t('batch.create.col.checks')}</th>
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row) => {
                      const srvErrs = serverErrorsByRow.get(row.row_index) ?? [];
                      const bad = row.errors.length > 0 || srvErrs.length > 0;
                      return (
                        <tr key={row.row_index} className={bad ? 'bg-bad-50' : ''}>
                          <td className="num text-ink-400">{row.row_index + 1}</td>
                          <td>
                            <Hash value={row.recipient_address} head={16} tail={8} />
                          </td>
                          <td className="cell-num">
                            <Amount value={row.amount} unit={null} />
                          </td>
                          <td className="max-w-[160px] truncate text-ink-500" title={row.memo}>
                            {row.memo || <span className="text-ink-300">—</span>}
                          </td>
                          <td className="text-[0.75rem]">
                            {!bad && <CheckCircle2 className="h-3.5 w-3.5 text-ok-600" />}
                            {row.errors.length > 0 && (
                              <span className="text-bad-700">{row.errors.join(', ')}</span>
                            )}
                            {srvErrs.length > 0 && (
                              <span className="text-bad-700">
                                {srvErrs.map((er) => `${er.field}: ${er.message}`).join('; ')}
                              </span>
                            )}
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            </>
          )}
        </Card>

        {error && <div className="alert alert-bad">{error}</div>}

        <div className="flex items-center justify-between gap-3 rounded-[10px] border border-line-200 bg-surface px-4 py-3">
          <p className="text-[0.75rem] text-ink-400">
            {counts.valid > 0
              ? t('batch.create.ready_summary', {
                  count: counts.valid,
                  amount: counts.total.toFixed(8).replace(/\.?0+$/, ''),
                })
              : t('batch.create.awaiting_csv')}
          </p>
          <div className="flex items-center gap-2">
            <button className="btn-ghost" onClick={() => navigate('/batch-transfers')}>
              {t('common.cancel')}
            </button>
            <button
              className="btn-primary"
              disabled={submitting || counts.valid === 0 || sourceWalletId === null}
              title={
                counts.valid === 0
                  ? t('batch.create.awaiting_csv')
                  : sourceWalletId === null
                    ? t('batch.create.err_no_wallet')
                    : undefined
              }
              onClick={onSubmit}
            >
              <Send className="h-4 w-4" />
              {submitting ? t('common.loading') : t('batch.create.submit')}
            </button>
          </div>
        </div>
      </div>
    </>
  );
}

function ModeOption({
  selected,
  onSelect,
  title,
  help,
}: {
  selected: boolean;
  onSelect: () => void;
  title: string;
  help: string;
}) {
  return (
    <label
      className={`flex cursor-pointer gap-2.5 rounded-[10px] border p-3.5 transition-colors ${
        selected
          ? 'border-brand-400 bg-brand-50 shadow-[0_0_0_3px_var(--color-brand-100)]'
          : 'border-line-200 bg-surface hover:border-line-300 hover:bg-surface-2'
      }`}
    >
      <input
        type="radio"
        className="mt-[3px] shrink-0 self-start accent-[var(--color-brand-600)]"
        checked={selected}
        onChange={onSelect}
      />
      <span>
        <span className="block text-[0.8125rem] font-medium text-ink-900">{title}</span>
        <span className="mt-1 block text-[0.75rem] leading-relaxed text-ink-500">{help}</span>
      </span>
    </label>
  );
}

/**
 * Minimal CSV parser — same dialect as the payroll importer. Skips empty
 * lines and a `recipient_address,...` header row. Client-side checks are
 * only a pre-filter; the server re-validates every row (Orchard-capable
 * unified address, dedup, balance).
 */
function parseCsv(text: string): BatchCsvRow[] {
  const out: BatchCsvRow[] = [];
  const lines = text.split(/\r?\n/);
  let rowIdx = 0;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (/^recipient_address\s*,/i.test(line)) continue;
    const cols = line.split(',').map((c) => c.trim().replace(/^"|"$/g, ''));
    const [address = '', amount = '', memo = ''] = cols;
    const errors: string[] = [];
    if (!address) errors.push('missing address');
    else if (!address.startsWith('u')) errors.push('not a unified address');
    if (!amount) errors.push('missing amount');
    else if (!/^\d+(\.\d+)?$/.test(amount) || Number(amount) <= 0) errors.push('invalid amount');
    out.push({ row_index: rowIdx++, recipient_address: address, amount, memo, errors });
  }
  return out;
}
