import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCw, XCircle } from 'lucide-react';
import { Card, LoadingSpinner, ExtendedStatusBadge } from '../../components/Common';
import { approvalService, transferService } from '../../services/api';
import type { Transfer } from '../../types';
import type { TransferStatusExt, TransferApprovalFields } from '../../types/approval';

/**
 * F2.1 §6 Page 2 — Maker view: transfers I created that are pending approval.
 * Skeleton: lists pending/approved/rejected/expired buckets, supports
 * self-recall (FR-13) while in AwaitingApproval state.
 */
type TransferWithApproval = Omit<Transfer, 'status'> & TransferApprovalFields & { status: TransferStatusExt };

export function MyPendingApprovals() {
  const { t } = useTranslation();
  const [list, setList] = useState<TransferWithApproval[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      // Reuse the existing /transfers endpoint; backend will return the
      // extended status alongside approval fields. Filter client-side until
      // a dedicated /my-pending endpoint exists.
      const data = await transferService.listTransfers(50, 0);
      const augmented = data.transfers as unknown as TransferWithApproval[];
      setList(augmented.filter(t => ['awaiting_approval', 'approved', 'rejected', 'expired'].includes(t.status)));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { load(); }, []);

  async function onRecall(id: number) {
    if (!confirm(t('approval.confirm_recall'))) return;
    try {
      await approvalService.recall(id);
      await load();
    } catch (e) {
      alert((e as Error).message);
    }
  }

  const grouped = {
    awaiting_approval: list.filter(x => x.status === 'awaiting_approval'),
    approved:          list.filter(x => x.status === 'approved'),
    rejected:          list.filter(x => x.status === 'rejected'),
    expired:           list.filter(x => x.status === 'expired'),
  };

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 space-y-6">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('approval.my_pending.title')}</h1>
        <button className="btn-ghost" onClick={load} aria-label={t('common.refresh')}>
          <RefreshCw className="w-4 h-4" />
        </button>
      </header>

      {error && (
        <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>
      )}

      {(['awaiting_approval', 'approved', 'rejected', 'expired'] as const).map(status => (
        <section key={status}>
          <h2 className="text-sm font-semibold text-gray-600 mb-2">
            {t(`approval.my_pending.section.${status}`)} ({grouped[status].length})
          </h2>
          {grouped[status].length === 0 ? (
            <p className="text-sm text-gray-400">{t('approval.empty')}</p>
          ) : (
            <ul className="space-y-2">
              {grouped[status].map(item => (
                <li key={item.id}>
                  <Card>
                    <div className="flex items-center justify-between">
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <ExtendedStatusBadge status={item.status} />
                          <span className="font-mono text-sm">#{item.id}</span>
                          <span className="text-sm">{item.amount} {item.token}</span>
                          <span className="text-xs text-gray-500 font-mono truncate max-w-xs">→ {item.to_address}</span>
                        </div>
                        {item.expiry_at && (
                          <p className="text-xs text-gray-500 mt-1">
                            {t('approval.expires_at')}: {new Date(item.expiry_at).toLocaleString()}
                          </p>
                        )}
                        {item.rejection_reason && (
                          <p className="text-xs text-red-700 mt-1">{t('approval.rejected_reason')}: {item.rejection_reason}</p>
                        )}
                      </div>
                      {item.status === 'awaiting_approval' && (
                        <button
                          className="btn-secondary"
                          onClick={() => onRecall(item.id)}
                          title={t('approval.recall')}
                        >
                          <XCircle className="w-4 h-4 inline mr-1" />
                          {t('approval.recall')}
                        </button>
                      )}
                    </div>
                  </Card>
                </li>
              ))}
            </ul>
          )}
        </section>
      ))}
    </div>
  );
}
