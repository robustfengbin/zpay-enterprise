import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { Upload, AlertTriangle, CheckCircle, Send, Shield } from 'lucide-react';
import { Card } from '../../components/Common';
import { batchTransferService } from '../../services/api/batch-transfer';
import { walletService } from '../../services/api';
import type {
  BatchCsvRow,
  BatchValidationError,
  PrivacyMode,
} from '../../types/batch-transfer';
import type { Wallet } from '../../types';

/**
 * F4.2 — Create a batch privacy transfer run (PRD-F4 §5).
 *
 * Flow (skeleton borrowed from Payroll RunCreate):
 *  1. Title + source Zcash wallet
 *  2. Upload CSV — parsed client-side. Columns: recipient_address, amount, memo
 *  3. Privacy scheduling: off (all at once) or staggered (batches over a
 *     window, optional per-transfer cap that splits big rows)
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
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [serverErrors, setServerErrors] = useState<BatchValidationError[]>([]);

  useEffect(() => {
    walletService.listWallets()
      .then(ws => setWallets(ws.filter(w => w.chain === 'zcash')))
      .catch(e => setError((e as Error).message));
  }, []);

  const counts = useMemo(() => {
    const valid = rows.filter(r => r.errors.length === 0).length;
    const total = rows
      .filter(r => r.errors.length === 0)
      .reduce((acc, r) => acc + Number(r.amount || 0), 0);
    return { rows: rows.length, valid, invalid: rows.length - valid, total };
  }, [rows]);

  // Server errors are keyed by the submitted (valid-rows-only) index; map
  // them back onto the preview rows so they highlight the right line.
  const serverErrorsByRow = useMemo(() => {
    const validRows = rows.filter(r => r.errors.length === 0);
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

  function onFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    if (!file) return;
    setError(null);
    setServerErrors([]);
    const reader = new FileReader();
    reader.onload = () => setRows(parseCsv(String(reader.result || '')));
    reader.onerror = () => setError(t('batch.create.csv_read_error'));
    reader.readAsText(file);
  }

  async function onSubmit() {
    if (!title.trim()) { setError(t('batch.create.err_no_title')); return; }
    if (sourceWalletId === null) { setError(t('batch.create.err_no_wallet')); return; }
    const items = rows
      .filter(r => r.errors.length === 0)
      .map(r => ({
        recipient_address: r.recipient_address,
        amount: r.amount,
        memo: r.memo || undefined,
      }));
    if (items.length === 0) { setError(t('batch.create.err_no_valid_rows')); return; }
    if (counts.invalid > 0 && !confirm(t('batch.create.confirm_with_invalid', { count: counts.invalid }))) return;

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

  const selectedWallet = wallets.find(w => w.id === sourceWalletId);

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold">{t('batch.create.title')}</h1>
      <p className="text-sm text-gray-500">{t('batch.create.subtitle')}</p>

      <Card>
        <div className="grid grid-cols-3 gap-3 text-sm">
          <label>
            <span className="text-gray-600">{t('batch.create.run_title')}</span>
            <input
              className="mt-1 w-full border rounded p-2"
              value={title}
              onChange={e => setTitle(e.target.value)}
              placeholder={t('batch.create.run_title_placeholder')}
              maxLength={120}
            />
          </label>
          <label>
            <span className="text-gray-600">{t('batch.create.source_wallet')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={sourceWalletId ?? ''}
              onChange={e => setSourceWalletId(e.target.value ? Number(e.target.value) : null)}
            >
              <option value="">{t('batch.create.pick_wallet')}</option>
              {wallets.map(w => (
                <option key={w.id} value={w.id}>{w.name} ({w.chain})</option>
              ))}
            </select>
          </label>
          <label>
            <span className="text-gray-600">{t('batch.create.notes')}</span>
            <input
              className="mt-1 w-full border rounded p-2"
              value={notes}
              onChange={e => setNotes(e.target.value)}
              placeholder={t('batch.create.notes_placeholder')}
            />
          </label>
        </div>
        {selectedWallet && (
          <p className="mt-2 text-xs text-gray-500 font-mono">{selectedWallet.address}</p>
        )}
      </Card>

      <Card>
        <div className="flex items-center gap-2 mb-2">
          <Shield className="w-4 h-4 text-blue-600" />
          <h2 className="font-semibold text-sm">{t('batch.create.privacy_title')}</h2>
        </div>
        <div className="grid grid-cols-2 gap-3 text-sm mb-3">
          <label
            className={`border rounded p-3 cursor-pointer ${privacyMode === 'off' ? 'border-blue-600 bg-blue-50' : 'border-gray-200'}`}
          >
            <input
              type="radio"
              className="mr-2"
              checked={privacyMode === 'off'}
              onChange={() => setPrivacyMode('off')}
            />
            <span className="font-medium">{t('batch.create.mode_off')}</span>
            <p className="text-xs text-gray-500 mt-1">{t('batch.create.mode_off_help')}</p>
          </label>
          <label
            className={`border rounded p-3 cursor-pointer ${privacyMode === 'staggered' ? 'border-blue-600 bg-blue-50' : 'border-gray-200'}`}
          >
            <input
              type="radio"
              className="mr-2"
              checked={privacyMode === 'staggered'}
              onChange={() => setPrivacyMode('staggered')}
            />
            <span className="font-medium">{t('batch.create.mode_staggered')}</span>
            <p className="text-xs text-gray-500 mt-1">{t('batch.create.mode_staggered_help')}</p>
          </label>
        </div>
        {privacyMode === 'staggered' && (
          <div className="grid grid-cols-3 gap-3 text-sm">
            <label>
              <span className="text-gray-600">{t('batch.create.batch_count')}</span>
              <input
                type="number"
                min={1}
                max={50}
                className="mt-1 w-full border rounded p-2"
                value={batchCount}
                onChange={e => setBatchCount(e.target.value)}
              />
            </label>
            <label>
              <span className="text-gray-600">{t('batch.create.window_hours')}</span>
              <input
                type="number"
                min={1}
                max={336}
                className="mt-1 w-full border rounded p-2"
                value={windowHours}
                onChange={e => setWindowHours(e.target.value)}
              />
            </label>
            <label>
              <span className="text-gray-600">{t('batch.create.max_per_transfer')}</span>
              <input
                className="mt-1 w-full border rounded p-2"
                value={maxPerTransfer}
                onChange={e => setMaxPerTransfer(e.target.value)}
                placeholder={t('batch.create.max_per_transfer_placeholder')}
              />
              <span className="text-xs text-gray-400">{t('batch.create.max_per_transfer_help')}</span>
            </label>
          </div>
        )}
      </Card>

      <Card>
        <div className="space-y-3">
          <p className="text-sm text-gray-600">{t('batch.create.csv_help')}</p>
          <p className="text-xs text-gray-500 font-mono">recipient_address, amount, memo</p>
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
            {t('batch.create.upload_csv')}
          </button>
          {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}
        </div>
      </Card>

      {rows.length > 0 && (
        <>
          <Card>
            <div className="flex items-center gap-4 text-sm">
              <span className="font-semibold">{t('batch.create.preview_summary')}:</span>
              <span>
                <CheckCircle className="w-4 h-4 inline text-green-600 mr-1" />
                {counts.valid} {t('batch.create.valid')}
              </span>
              {counts.invalid > 0 && (
                <span className="text-red-600">
                  <AlertTriangle className="w-4 h-4 inline mr-1" />
                  {counts.invalid} {t('batch.create.invalid')}
                </span>
              )}
              <span className="text-gray-500">
                {t('batch.create.total_amount')}: <span className="font-mono">{counts.total.toFixed(8)}</span> ZEC
              </span>
            </div>
          </Card>

          <Card>
            <table className="w-full text-xs">
              <thead className="text-gray-500 uppercase">
                <tr>
                  <th className="text-left p-2">#</th>
                  <th className="text-left p-2">{t('batch.create.col.address')}</th>
                  <th className="text-right p-2">{t('batch.create.col.amount')}</th>
                  <th className="text-left p-2">{t('batch.create.col.memo')}</th>
                  <th className="text-left p-2">{t('batch.create.col.checks')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map(row => {
                  const srvErrs = serverErrorsByRow.get(row.row_index) ?? [];
                  const bad = row.errors.length > 0 || srvErrs.length > 0;
                  return (
                    <tr key={row.row_index} className={`border-t border-gray-100 ${bad ? 'bg-red-50' : ''}`}>
                      <td className="p-2">{row.row_index + 1}</td>
                      <td className="p-2 font-mono truncate max-w-[260px]" title={row.recipient_address}>
                        {row.recipient_address}
                      </td>
                      <td className="p-2 text-right font-mono">{row.amount}</td>
                      <td className="p-2">{row.memo || '—'}</td>
                      <td className="p-2">
                        {!bad && <span className="text-green-700 text-xs">✓</span>}
                        {row.errors.length > 0 && (
                          <span className="text-red-700 text-xs">{row.errors.join(', ')}</span>
                        )}
                        {srvErrs.length > 0 && (
                          <span className="text-red-700 text-xs">
                            {srvErrs.map(er => `${er.field}: ${er.message}`).join('; ')}
                          </span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </Card>

          <div className="flex gap-2">
            <button
              className="btn-primary"
              disabled={submitting || counts.valid === 0 || sourceWalletId === null}
              onClick={onSubmit}
            >
              <Send className="w-4 h-4 inline mr-1" />
              {submitting ? t('common.loading') : t('batch.create.submit')}
            </button>
            <button className="btn-ghost" onClick={() => navigate('/batch-transfers')}>
              {t('common.cancel')}
            </button>
          </div>
        </>
      )}
    </div>
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
    const cols = line.split(',').map(c => c.trim().replace(/^"|"$/g, ''));
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
