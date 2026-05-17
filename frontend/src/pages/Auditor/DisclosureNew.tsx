import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams, useNavigate } from 'react-router-dom';
import { FileText, RefreshCw, AlertTriangle } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type {
  DisclosureBody,
  DisclosureCreateResponse,
  DisclosureFormat,
  DisclosureGranularity,
  DisclosureRequest,
  DisclosureRow,
} from '../../types/viewing-key';

/**
 * F1.1 §5 — Generate ZIP-307-inspired payment disclosure.
 *
 * Backend is async (returns 202 with status:"generating"). Flow:
 *  1. POST /wallets/{id}/payment-disclosures → disclosure_id + status
 *  2. Poll GET /payment-disclosures/{id} every 2s until status='ready'|'failed'
 *  3. GET /payment-disclosures/{id}/download → real DisclosureBody
 *  4. Render actions[] table with ZEC + zatoshi double-format, nullifier, etc.
 */
export function DisclosureNew() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const initialWalletId = Number(params.get('wallet_id')) || 0;

  // ----- Form state -----
  const [walletId, setWalletId] = useState<number>(initialWalletId);
  const [granularity, setGranularity] = useState<DisclosureGranularity>('range');
  const [txHash, setTxHash] = useState('');
  const [address, setAddress] = useState('');
  const [fromValue, setFromValue] = useState('');
  const [toValue, setToValue] = useState('');
  const [rangeMode, setRangeMode] = useState<'timestamp' | 'height'>('timestamp');
  const [format, setFormat] = useState<DisclosureFormat>('json');

  // ----- Async state -----
  const [creating, setCreating] = useState(false);
  const [row, setRow] = useState<DisclosureRow | null>(null);
  const [body, setBody] = useState<DisclosureBody | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pollTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => { if (pollTimer.current) clearTimeout(pollTimer.current); };
  }, []);

  function buildScopeParam(): DisclosureRequest['scope_param'] | null {
    if (granularity === 'tx') {
      if (!txHash.trim()) { setError(t('auditor.disclosure.err_tx_hash_required')); return null; }
      return { tx_hash: txHash.trim() };
    }
    if (granularity === 'address') {
      if (!address.trim()) { setError(t('auditor.disclosure.err_address_required')); return null; }
      return { address: address.trim() };
    }
    // range: pass timestamp string OR u64 height — backend auto-detects (b0793fd).
    if (!fromValue || !toValue) { setError(t('auditor.disclosure.err_range_required')); return null; }
    const parseValue = (v: string): string | number => {
      if (rangeMode === 'height') {
        const n = Number(v);
        if (!Number.isFinite(n) || n < 0) throw new Error('invalid block height');
        return n;
      }
      // ISO 8601 — input type="datetime-local" yields "2026-05-17T12:34", append Z
      return v.length > 10 && !v.endsWith('Z') ? `${v}:00Z`.replace(/::00Z$/, ':00Z') : v;
    };
    try {
      return { from: parseValue(fromValue), to: parseValue(toValue) };
    } catch (e) {
      setError((e as Error).message);
      return null;
    }
  }

  async function onGenerate() {
    setError(null);
    setBody(null);
    setRow(null);
    if (!walletId) { setError(t('auditor.disclosure.err_wallet_required')); return; }
    const scope = buildScopeParam();
    if (!scope) return;
    setCreating(true);
    try {
      const resp: DisclosureCreateResponse = await viewingKeyService.generateDisclosure(walletId, {
        granularity,
        scope_param: scope,
        format,
      });
      // Optimistically render a "generating" row so polling has something to overlay.
      setRow({
        id: resp.disclosure_id,
        wallet_id: walletId,
        generated_by_user_id: 0,
        granularity,
        scope_param: scope,
        tx_count: resp.tx_count,
        disclosure_json: null,
        format,
        file_path: null,
        status: resp.status,
        error_message: null,
        expires_at: resp.expires_at,
        created_at: resp.created_at,
      });
      schedulePoll(resp.disclosure_id);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setCreating(false);
    }
  }

  function schedulePoll(id: number) {
    if (pollTimer.current) clearTimeout(pollTimer.current);
    pollTimer.current = setTimeout(() => void pollOnce(id), 2000);
  }

  async function pollOnce(id: number) {
    try {
      const fresh = await viewingKeyService.getDisclosure(id);
      setRow(fresh);
      if (fresh.status === 'ready') {
        const dl = await viewingKeyService.downloadDisclosure(id);
        setBody(dl);
        return;
      }
      if (fresh.status === 'failed') {
        setError(fresh.error_message || t('auditor.disclosure.err_generation_failed'));
        return;
      }
      schedulePoll(id);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  // ----- Render: result view -----
  if (row && (body || row.status !== 'generating')) {
    return (
      <DisclosureResult
        row={row}
        body={body}
        onBack={() => navigate('/auditor')}
        onReset={() => { setRow(null); setBody(null); setError(null); }}
      />
    );
  }

  // ----- Render: generating spinner -----
  if (row && row.status === 'generating') {
    return (
      <div className="p-6 max-w-2xl space-y-4">
        <h1 className="text-2xl font-semibold flex items-center gap-2">
          <FileText className="w-6 h-6" />
          {t('auditor.disclosure.generating_title')}
        </h1>
        <Card>
          <div className="flex items-center gap-3">
            <LoadingSpinner />
            <div className="text-sm">
              <p>{t('auditor.disclosure.generating_body', { id: row.id })}</p>
              <p className="text-xs text-gray-500 mt-1">
                {t('auditor.disclosure.poll_hint')}
              </p>
            </div>
          </div>
        </Card>
      </div>
    );
  }

  // ----- Render: form -----
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
              value={walletId || ''}
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
              <option value="tx">{t('auditor.disclosure.granularity.tx')}</option>
              <option value="address">{t('auditor.disclosure.granularity.address')}</option>
              <option value="range">{t('auditor.disclosure.granularity.range')}</option>
            </select>
          </label>

          {granularity === 'tx' && (
            <label className="block">
              <span className="text-gray-600">{t('auditor.disclosure.tx_hash')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={txHash} onChange={e => setTxHash(e.target.value)} />
            </label>
          )}

          {granularity === 'address' && (
            <label className="block">
              <span className="text-gray-600">{t('auditor.disclosure.address')}</span>
              <input className="mt-1 w-full border rounded p-2 font-mono text-xs" value={address} onChange={e => setAddress(e.target.value)} />
            </label>
          )}

          {granularity === 'range' && (
            <>
              <div className="flex gap-3 text-xs">
                <label className="flex items-center gap-1">
                  <input
                    type="radio"
                    name="range_mode"
                    checked={rangeMode === 'timestamp'}
                    onChange={() => setRangeMode('timestamp')}
                  />
                  {t('auditor.disclosure.range_mode_timestamp')}
                </label>
                <label className="flex items-center gap-1">
                  <input
                    type="radio"
                    name="range_mode"
                    checked={rangeMode === 'height'}
                    onChange={() => setRangeMode('height')}
                  />
                  {t('auditor.disclosure.range_mode_height')}
                </label>
              </div>
              <div className="grid grid-cols-2 gap-3">
                <label>
                  <span className="text-gray-600">{t('auditor.disclosure.start_at')}</span>
                  <input
                    type={rangeMode === 'timestamp' ? 'datetime-local' : 'number'}
                    className="mt-1 w-full border rounded p-2 font-mono text-xs"
                    value={fromValue}
                    onChange={e => setFromValue(e.target.value)}
                    placeholder={rangeMode === 'height' ? '2400000' : ''}
                  />
                </label>
                <label>
                  <span className="text-gray-600">{t('auditor.disclosure.end_at')}</span>
                  <input
                    type={rangeMode === 'timestamp' ? 'datetime-local' : 'number'}
                    className="mt-1 w-full border rounded p-2 font-mono text-xs"
                    value={toValue}
                    onChange={e => setToValue(e.target.value)}
                    placeholder={rangeMode === 'height' ? '2500000' : ''}
                  />
                </label>
              </div>
            </>
          )}

          <label className="block">
            <span className="text-gray-600">{t('auditor.disclosure.format')}</span>
            <select
              className="mt-1 w-full border rounded p-2"
              value={format}
              onChange={e => setFormat(e.target.value as DisclosureFormat)}
            >
              <option value="json">JSON</option>
              <option value="csv">CSV {t('auditor.disclosure.format_m2_note')}</option>
              <option value="pdf">PDF {t('auditor.disclosure.format_m2_note')}</option>
            </select>
          </label>
        </div>

        {error && (
          <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm mt-3 flex items-start gap-2">
            <AlertTriangle className="w-4 h-4 mt-0.5 shrink-0" />
            <span>{error}</span>
          </div>
        )}

        <div className="flex gap-2 mt-4">
          <button className="btn-primary" disabled={creating || !walletId} onClick={onGenerate}>
            {creating ? t('common.loading') : t('auditor.disclosure.generate')}
          </button>
          <button className="btn-ghost" onClick={() => navigate('/auditor')}>{t('common.cancel')}</button>
        </div>
      </Card>
    </div>
  );
}

interface DisclosureResultProps {
  row: DisclosureRow;
  body: DisclosureBody | null;
  onBack: () => void;
  onReset: () => void;
}

function DisclosureResult({ row, body, onBack, onReset }: DisclosureResultProps) {
  const { t } = useTranslation();
  const failed = row.status === 'failed';
  return (
    <div className="p-6 max-w-5xl space-y-4">
      <h1 className="text-2xl font-semibold flex items-center gap-2">
        <FileText className="w-6 h-6" />
        {failed ? t('auditor.disclosure.failed_title') : t('auditor.disclosure.generated_title')}
      </h1>

      <Card>
        <dl className="grid grid-cols-3 gap-y-2 text-sm">
          <dt className="text-gray-500">{t('auditor.disclosure.id')}</dt>
          <dd className="font-mono col-span-2">#{row.id}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.granularity')}</dt>
          <dd className="col-span-2">{t(`auditor.disclosure.granularity.${row.granularity as 'tx' | 'address' | 'range'}`)}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.status')}</dt>
          <dd className="col-span-2">
            <span className={`text-xs px-1.5 py-0.5 rounded ${row.status === 'ready' ? 'bg-green-100 text-green-800' : 'bg-red-100 text-red-800'}`}>
              {t(`auditor.disclosure.status_${row.status}`)}
            </span>
          </dd>
          <dt className="text-gray-500">{t('auditor.disclosure.items_count')}</dt>
          <dd className="col-span-2">{body?.action_count ?? row.tx_count}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.generated_at')}</dt>
          <dd className="col-span-2">{new Date(row.created_at).toLocaleString()}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.expires_at')}</dt>
          <dd className="col-span-2">{new Date(row.expires_at).toLocaleString()}</dd>
          {row.error_message && (
            <>
              <dt className="text-gray-500">{t('auditor.disclosure.error')}</dt>
              <dd className="col-span-2 text-red-700">{row.error_message}</dd>
            </>
          )}
        </dl>
      </Card>

      {body?.resolved_range && (
        <Card>
          <h2 className="font-semibold text-sm mb-2">{t('auditor.disclosure.resolved_range')}</h2>
          <dl className="grid grid-cols-4 gap-y-2 text-sm">
            <dt className="text-gray-500">{t('auditor.disclosure.from_ts')}</dt>
            <dd className="text-xs">{new Date(body.resolved_range.from_ts).toLocaleString()}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.to_ts')}</dt>
            <dd className="text-xs">{new Date(body.resolved_range.to_ts).toLocaleString()}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.from_height')}</dt>
            <dd className="font-mono text-xs">{body.resolved_range.from_height}</dd>
            <dt className="text-gray-500">{t('auditor.disclosure.to_height')}</dt>
            <dd className="font-mono text-xs">{body.resolved_range.to_height}</dd>
          </dl>
        </Card>
      )}

      {body && body.actions.length > 0 && (
        <Card>
          <h2 className="font-semibold mb-2 text-sm">
            {t('auditor.disclosure.actions_title')}
            <span className="ml-2 text-xs text-gray-500 font-normal">{body.zip_version}</span>
          </h2>
          <table className="w-full text-xs">
            <thead className="text-gray-500 uppercase">
              <tr>
                <th className="text-left p-2">{t('auditor.disclosure.col.tx_hash')}</th>
                <th className="text-right p-2">{t('auditor.disclosure.col.block_height')}</th>
                <th className="text-right p-2">{t('auditor.disclosure.col.value_zec')}</th>
                <th className="text-right p-2">{t('auditor.disclosure.col.value_zatoshi')}</th>
                <th className="text-left p-2">{t('auditor.disclosure.col.recipient')}</th>
                <th className="text-left p-2">{t('auditor.disclosure.col.memo')}</th>
                <th className="text-left p-2">{t('auditor.disclosure.col.spent')}</th>
              </tr>
            </thead>
            <tbody>
              {body.actions.map((a, i) => (
                <tr key={i} className="border-t border-gray-100">
                  <td className="p-2 font-mono truncate max-w-[160px]" title={a.tx_hash}>{a.tx_hash}</td>
                  <td className="p-2 text-right font-mono">{a.block_height}</td>
                  <td className="p-2 text-right font-mono">{a.value_zec.toFixed(8)}</td>
                  <td className="p-2 text-right font-mono text-gray-500">{a.value_zatoshis}</td>
                  <td className="p-2 font-mono truncate max-w-[180px]" title={a.recipient_address_hex || ''}>{a.recipient_address_hex || '—'}</td>
                  <td className="p-2 truncate max-w-[160px]">{a.memo || '—'}</td>
                  <td className="p-2">
                    {a.is_spent
                      ? <span title={a.spent_in_tx || ''} className="text-red-700">{t('auditor.disclosure.spent_yes')}</span>
                      : <span className="text-gray-500">{t('auditor.disclosure.spent_no')}</span>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p className="text-xs text-gray-500 mt-2">{t('auditor.disclosure.actions_note')}</p>
        </Card>
      )}

      {body && body.actions.length === 0 && (
        <Card>
          <p className="text-sm text-gray-600">{t('auditor.disclosure.empty_actions')}</p>
        </Card>
      )}

      <div className="flex gap-2">
        <button className="btn-secondary" onClick={onReset}>
          <RefreshCw className="w-4 h-4 inline mr-1" /> {t('auditor.disclosure.action_new')}
        </button>
        <button className="btn-ghost" onClick={onBack}>{t('common.back')}</button>
      </div>
    </div>
  );
}
