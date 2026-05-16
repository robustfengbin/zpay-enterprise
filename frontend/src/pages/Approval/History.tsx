import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams } from 'react-router-dom';
import { Card, LoadingSpinner, ExtendedStatusBadge, ApprovalTimeline, buildTimelineFromApprovals } from '../../components/Common';
import { approvalService, transferService } from '../../services/api';
import type { Transfer } from '../../types';
import type { TransferStatusExt, TransferApprovalFields, TransferApproval } from '../../types/approval';

type TransferDetail = Omit<Transfer, 'status'> & TransferApprovalFields & { status: TransferStatusExt };

/**
 * F2.1 §6 Page 5 — Transfer history detail with approval timeline.
 * Read-only view, shows every state transition in chronological order.
 */
export function ApprovalHistory() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const [transfer, setTransfer] = useState<TransferDetail | null>(null);
  const [approvals, setApprovals] = useState<TransferApproval[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const [t1, t2] = await Promise.all([
          transferService.getTransfer(Number(id)),
          approvalService.getHistory(Number(id)),
        ]);
        setTransfer(t1 as unknown as TransferDetail);
        setApprovals(t2);
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, [id]);

  if (loading) return <LoadingSpinner />;
  if (!transfer) return <div className="p-6 text-red-600">{error || t('common.not_found')}</div>;

  // Construct transaction-side events from the existing transfer record
  const txEvents: { ts: string; kind: 'execute' | 'confirm' | 'fail'; detail?: string }[] = [];
  if (['submitted', 'confirmed', 'failed'].includes(transfer.status)) {
    txEvents.push({ ts: transfer.updated_at, kind: 'execute' });
  }
  if (transfer.status === 'confirmed' && transfer.tx_hash) {
    txEvents.push({ ts: transfer.updated_at, kind: 'confirm', detail: transfer.tx_hash });
  }
  if (transfer.status === 'failed' && transfer.error_message) {
    txEvents.push({ ts: transfer.updated_at, kind: 'fail', detail: transfer.error_message });
  }

  // Maker username is on the transfer in the augmented response; fallback to id.
  const makerLabel = (transfer as unknown as { maker_username?: string }).maker_username
    || `user#${transfer.initiated_by}`;

  const events = buildTimelineFromApprovals(approvals, makerLabel, transfer.created_at, txEvents);

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('approval.history.title')} #{transfer.id}</h1>
        <ExtendedStatusBadge status={transfer.status} />
      </header>

      <Card>
        <dl className="grid grid-cols-2 gap-y-3 text-sm">
          <dt className="text-gray-500">{t('approval.detail.amount')}</dt>
          <dd className="font-mono font-medium">{transfer.amount} {transfer.token}</dd>
          <dt className="text-gray-500">{t('approval.detail.to')}</dt>
          <dd className="font-mono text-xs truncate">{transfer.to_address}</dd>
          {transfer.tx_hash && (
            <>
              <dt className="text-gray-500">{t('transfer.tx_hash')}</dt>
              <dd className="font-mono text-xs truncate">{transfer.tx_hash}</dd>
            </>
          )}
        </dl>
      </Card>

      <Card>
        <h2 className="font-semibold mb-3">{t('approval.history.timeline')}</h2>
        <ApprovalTimeline events={events} />
      </Card>
    </div>
  );
}
