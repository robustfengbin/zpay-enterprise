import React, { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, FileText, RefreshCw } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type {
  DisclosureBody,
  DisclosureRow,
} from '../../types/viewing-key';

/**
 * F1.1 §5 — Single disclosure detail. Loads the row by id; if still
 * generating, polls every 2s until ready/failed and then pulls the body.
 */
export function DisclosureDetail() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const did = Number(id);

  const [row, setRow] = useState<DisclosureRow | null>(null);
  const [body, setBody] = useState<DisclosureBody | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const pollTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => { if (pollTimer.current) clearTimeout(pollTimer.current); };
  }, []);

  useEffect(() => { void load(); /* eslint-disable-next-line */ }, [did]);

  async function load() {
    if (!Number.isFinite(did)) { setError(t('common.not_found')); setLoading(false); return; }
    setLoading(true);
    try {
      const fresh = await viewingKeyService.getDisclosure(did);
      setRow(fresh);
      if (fresh.status === 'ready') {
        const dl = await viewingKeyService.downloadDisclosure(did);
        setBody(dl);
      } else if (fresh.status === 'generating') {
        schedulePoll();
      } else if (fresh.status === 'failed') {
        setError(fresh.error_message || t('auditor.disclosure.err_generation_failed'));
      }
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  function schedulePoll() {
    if (pollTimer.current) clearTimeout(pollTimer.current);
    pollTimer.current = setTimeout(() => void load(), 2000);
  }

  if (loading && !row) return <LoadingSpinner />;
  if (!row) return <div className="p-6 text-red-600">{error || t('common.not_found')}</div>;

  return (
    <div className="p-6 max-w-5xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <button className="btn-ghost mb-2" onClick={() => navigate(-1)}>
            <ArrowLeft className="w-4 h-4 inline mr-1" /> {t('common.back')}
          </button>
          <h1 className="text-2xl font-semibold flex items-center gap-2">
            <FileText className="w-6 h-6" />
            {t('auditor.disclosure.detail_title')} #{row.id}
          </h1>
        </div>
        <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      <Card>
        <dl className="grid grid-cols-3 gap-y-2 text-sm">
          <dt className="text-gray-500">{t('auditor.disclosure.status')}</dt>
          <dd className="col-span-2">
            <span className={`text-xs px-1.5 py-0.5 rounded ${statusClass(row.status)}`}>
              {t(`auditor.disclosure.status_${row.status}`)}
            </span>
          </dd>
          <dt className="text-gray-500">{t('auditor.disclosure.granularity')}</dt>
          <dd className="col-span-2">{t(`auditor.disclosure.granularity.${row.granularity as 'tx' | 'address' | 'range'}`)}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.items_count')}</dt>
          <dd className="col-span-2">{body?.action_count ?? row.tx_count}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.format')}</dt>
          <dd className="col-span-2 uppercase">{row.format}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.generated_at')}</dt>
          <dd className="col-span-2">{new Date(row.created_at).toLocaleString()}</dd>
          <dt className="text-gray-500">{t('auditor.disclosure.expires_at')}</dt>
          <dd className="col-span-2">{new Date(row.expires_at).toLocaleString()}</dd>
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

      {row.status === 'generating' && (
        <Card>
          <div className="flex items-center gap-3 text-sm">
            <LoadingSpinner />
            <p>{t('auditor.disclosure.generating_body', { id: row.id })}</p>
          </div>
        </Card>
      )}
    </div>
  );
}

function statusClass(s: string): string {
  switch (s) {
    case 'ready':      return 'bg-green-100 text-green-800';
    case 'generating': return 'bg-blue-100 text-blue-800';
    case 'failed':     return 'bg-red-100 text-red-800';
    default:           return 'bg-gray-100 text-gray-700';
  }
}
