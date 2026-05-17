import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { RefreshCw, Bell } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { approvalService } from '../../services/api';
import type { PendingApprovalItem } from '../../types/approval';

/**
 * F2.1 §6 Page 3 — Checker view: queue of transfers awaiting my approval.
 * Self-filtering: backend excludes transfers where the caller is the maker
 * per NFR-7 (maker != checker).
 */
export function ApprovalQueue() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [items, setItems] = useState<PendingApprovalItem[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load(reset = true) {
    setLoading(true);
    setError(null);
    try {
      const data = await approvalService.listPending(20, reset ? undefined : (cursor ?? undefined));
      setItems(reset ? data.items : [...items, ...data.items]);
      setCursor(data.next_cursor);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, []);

  function timeRemaining(expiry: string): { text: string; urgent: boolean } {
    const ms = new Date(expiry).getTime() - Date.now();
    if (ms <= 0) return { text: t('approval.expired'), urgent: true };
    const hours = Math.floor(ms / 3_600_000);
    const minutes = Math.floor((ms % 3_600_000) / 60_000);
    return {
      text: `${hours}h ${minutes}m`,
      urgent: hours < 2,
    };
  }

  if (loading && items.length === 0) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-4">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold flex items-center gap-2">
          <Bell className="w-6 h-6 text-yellow-500" />
          {t('approval.queue.title')}
          <span className="text-base font-normal text-gray-500">({items.length})</span>
        </h1>
        <button className="btn-ghost" onClick={() => load(true)} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      {error && (
        <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>
      )}

      {items.length === 0 ? (
        <Card>
          <p className="text-center text-gray-400 py-8">{t('approval.queue.empty')}</p>
        </Card>
      ) : (
        <Card>
          <table className="w-full">
            <thead className="text-xs text-gray-500 uppercase tracking-wide">
              <tr>
                <th className="text-left p-2">#</th>
                <th className="text-left p-2">{t('approval.queue.col.amount')}</th>
                <th className="text-left p-2">{t('approval.queue.col.token')}</th>
                <th className="text-left p-2">{t('approval.queue.col.maker')}</th>
                <th className="text-left p-2">{t('approval.queue.col.remaining')}</th>
                <th className="text-right p-2">{t('common.action')}</th>
              </tr>
            </thead>
            <tbody>
              {items.map(item => {
                const tr = timeRemaining(item.expiry_at);
                return (
                  <tr key={item.transfer_id} className="border-t border-gray-100 hover:bg-gray-50">
                    <td className="p-2 font-mono text-xs">{item.transfer_id}</td>
                    <td className="p-2">{item.amount}</td>
                    <td className="p-2 font-medium">{item.token}</td>
                    <td className="p-2 text-sm">{item.maker_username}</td>
                    <td className={`p-2 text-sm ${tr.urgent ? 'text-red-600 font-semibold' : ''}`}>
                      {tr.text}{tr.urgent && ' ⚠️'}
                    </td>
                    <td className="p-2 text-right">
                      <button
                        className="btn-secondary"
                        onClick={() => navigate(`/approval/${item.transfer_id}`)}
                      >
                        {t('approval.queue.review')}
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </Card>
      )}

      {cursor && (
        <button className="btn-ghost mx-auto block" onClick={() => load(false)}>
          {t('common.load_more')}
        </button>
      )}
    </div>
  );
}
