import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams, useNavigate } from 'react-router-dom';
import { FileText, Download } from 'lucide-react';
import { Card } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type { DisclosureGranularity, DisclosureFormat, DisclosureResponse } from '../../types/viewing-key';

/**
 * F1.1 §5 — Generate ZIP-307 payment disclosure with 3-tier granularity:
 *   - single_tx: a single transaction hash
 *   - single_address: all transactions for one address
 *   - time_range: all transactions in a date window
 * Output formats: PDF / CSV / JSON.
 */
export function DisclosureNew() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const initialWalletId = Number(params.get('wallet_id')) || 0;

  const [walletId, setWalletId] = useState<number>(initialWalletId);
  const [granularity, setGranularity] = useState<DisclosureGranularity>('time_range');
  const [txHash, setTxHash] = useState('');
  const [address, setAddress] = useState('');
  const [startAt, setStartAt] = useState('');
  const [endAt, setEndAt] = useState('');
  const [format, setFormat] = useState<DisclosureFormat>('pdf');
  const [submitting, setSubmitting] = useState(false);
  const [result, setResult] = useState<DisclosureResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function onGenerate() {
    setSubmitting(true);
    setError(null);
    try {
      const data = await viewingKeyService.generateDisclosure({
        wallet_id: walletId,
        granularity,
        tx_hash: granularity === 'single_tx' ? txHash : undefined,
        address: granularity === 'single_address' ? address : undefined,
        start_at: granularity === 'time_range' ? startAt : undefined,
        end_at: granularity === 'time_range' ? endAt : undefined,
        format,
      });
      setResult(data);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  if (result) {
    return (
      <div className="p-6 max-w-2xl space-y-4">
        <h1 className="text-2xl font-semibold flex items-center gap-2">
          <FileText className="w-6 h-6" />
          {t('auditor.disclosure.generated_title')}
        </h1>
        <Card>
          <dl className="grid grid-cols-2 gap-y-2 text-sm">
            <dt className="text-gray-500">{t('auditor.disclosure.id')}</dt>
            <dd className="font-mono text-xs">{result.disclosure_id}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.granularity')}</dt>
            <dd>{t(`auditor.disclosure.granularity.${result.granularity}`)}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.items_count')}</dt>
            <dd>{result.items.length}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.generated_at')}</dt>
            <dd>{new Date(result.generated_at).toLocaleString()}</dd>
          </dl>
        </Card>

        {result.download_url && (
          <a href={result.download_url} className="btn-primary inline-flex items-center gap-1">
            <Download className="w-4 h-4" />
            {t('auditor.disclosure.download')} ({format.toUpperCase()})
          </a>
        )}

        <Card>
          <h2 className="font-semibold mb-2 text-sm">{t('auditor.disclosure.preview')}</h2>
          <table className="w-full text-xs">
            <thead className="text-gray-500">
              <tr>
                <th className="text-left p-1">tx_hash</th>
                <th className="text-left p-1">block</th>
                <th className="text-left p-1">amount (zat)</th>
                <th className="text-left p-1">recipient</th>
                <th className="text-left p-1">memo</th>
              </tr>
            </thead>
            <tbody>
              {result.items.slice(0, 20).map((item, idx) => (
                <tr key={idx} className="border-t border-gray-100">
                  <td className="p-1 font-mono truncate max-w-[150px]">{item.tx_hash}</td>
                  <td className="p-1">{item.block_height}</td>
                  <td className="p-1 font-mono">{item.amount_zatoshi}</td>
                  <td className="p-1 font-mono truncate max-w-[180px]">{item.recipient_address || '—'}</td>
                  <td className="p-1 truncate max-w-[120px]">{item.memo_decoded || '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {result.items.length > 20 && (
            <p className="text-xs text-gray-500 mt-1">… {result.items.length - 20} {t('auditor.disclosure.more_items')}</p>
          )}
        </Card>

        <button className="btn-ghost" onClick={() => navigate('/auditor')}>{t('common.back')}</button>
      </div>
    );
  }

  return (
    <div className="p-6 max-w-2xl space-y-4">
      <h1 className="text-2xl font-semibold flex items-center gap-2">
        <FileText className="w-6 h-6" />
        {t('auditor.disclosure.new_title')}
      </h1>

      <Card>
        <div className="space-y-3 text-sm">
          <label className="block">
            <span className="text-gray-600">{t('auditor.disclosure.wallet_id')}</span>
            <input
              type="number"
              className="mt-1 w-full border rounded p-2"
              value={walletId}
              onChange={e => setWalletId(Number(e.target.value))}
            />
          </label>

          <label className="block">
            <span className="text-gray-600">{t('auditor.disclosure.granularity')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={granularity}
              onChange={e => setGranularity(e.target.value as DisclosureGranularity)}
            >
              <option value="single_tx">{t('auditor.disclosure.granularity.single_tx')}</option>
              <option value="single_address">{t('auditor.disclosure.granularity.single_address')}</option>
              <option value="time_range">{t('auditor.disclosure.granularity.time_range')}</option>
            </select>
          </label>

          {granularity === 'single_tx' && (
            <label className="block">
              <span className="text-gray-600">{t('auditor.disclosure.tx_hash')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={txHash} onChange={e => setTxHash(e.target.value)} />
            </label>
          )}

          {granularity === 'single_address' && (
            <label className="block">
              <span className="text-gray-600">{t('auditor.disclosure.address')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={address} onChange={e => setAddress(e.target.value)} />
            </label>
          )}

          {granularity === 'time_range' && (
            <div className="grid grid-cols-2 gap-3">
              <label>
                <span className="text-gray-600">{t('auditor.disclosure.start_at')}</span>
                <input type="datetime-local" className="mt-1 w-full border rounded p-2" value={startAt} onChange={e => setStartAt(e.target.value)} />
              </label>
              <label>
                <span className="text-gray-600">{t('auditor.disclosure.end_at')}</span>
                <input type="datetime-local" className="mt-1 w-full border rounded p-2" value={endAt} onChange={e => setEndAt(e.target.value)} />
              </label>
            </div>
          )}

          <label className="block">
            <span className="text-gray-600">{t('auditor.disclosure.format')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={format}
              onChange={e => setFormat(e.target.value as DisclosureFormat)}
            >
              <option value="pdf">PDF</option>
              <option value="csv">CSV</option>
              <option value="json">JSON</option>
            </select>
          </label>
        </div>

        {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm mt-3">{error}</div>}

        <div className="flex gap-2 mt-4">
          <button className="btn-primary" disabled={submitting || !walletId} onClick={onGenerate}>
            {t('auditor.disclosure.generate')}
          </button>
          <button className="btn-ghost" onClick={() => navigate('/auditor')}>{t('common.cancel')}</button>
        </div>
      </Card>
    </div>
  );
}
