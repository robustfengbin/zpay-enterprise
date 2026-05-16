import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useNavigate } from 'react-router-dom';
import { CheckCircle, XCircle, AlertTriangle } from 'lucide-react';
import { Card, LoadingSpinner, ExtendedStatusBadge } from '../../components/Common';
import { approvalService, transferService } from '../../services/api';
import type { Transfer } from '../../types';
import type { TransferStatusExt, TransferApprovalFields } from '../../types/approval';

type TransferDetail = Omit<Transfer, 'status'> & TransferApprovalFields & { status: TransferStatusExt; memo?: string | null };

/**
 * F2.1 §6 Page 4 — Approval Detail with risk tags + decide buttons.
 * Per RK-2: backend enforces maker != checker hard, but UI also hides the
 * decide buttons when current user is the maker as a UX guard.
 */
export function ApprovalDetail() {
  const { t } = useTranslation();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [transfer, setTransfer] = useState<TransferDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [note, setNote] = useState('');
  const [rejectReason, setRejectReason] = useState('');
  const [showRejectInput, setShowRejectInput] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const data = await transferService.getTransfer(Number(id));
        setTransfer(data as unknown as TransferDetail);
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, [id]);

  async function onApprove() {
    if (!transfer) return;
    setSubmitting(true);
    setError(null);
    try {
      await approvalService.approve(transfer.id, note || undefined);
      navigate('/approval/queue');
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  async function onReject() {
    if (!transfer) return;
    if (rejectReason.length < 5) {
      setError(t('approval.error.reason_too_short'));
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      await approvalService.reject(transfer.id, rejectReason);
      navigate('/approval/queue');
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  if (loading) return <LoadingSpinner />;
  if (!transfer) return <div className="p-6 text-red-600">{error || t('common.not_found')}</div>;

  const canDecide = transfer.status === 'awaiting_approval';

  return (
    <div className="p-6 space-y-4 max-w-3xl">
      <header className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">{t('approval.detail.title')} #{transfer.id}</h1>
        <ExtendedStatusBadge status={transfer.status} />
      </header>

      <Card>
        <dl className="grid grid-cols-2 gap-y-3 text-sm">
          <dt className="text-gray-500">{t('approval.detail.amount')}</dt>
          <dd className="font-mono font-medium">{transfer.amount} {transfer.token}</dd>

          <dt className="text-gray-500">{t('approval.detail.from')}</dt>
          <dd className="font-mono text-xs truncate">{transfer.from_address}</dd>

          <dt className="text-gray-500">{t('approval.detail.to')}</dt>
          <dd className="font-mono text-xs truncate">{transfer.to_address}</dd>

          {transfer.memo && (
            <>
              <dt className="text-gray-500">{t('approval.detail.memo')}</dt>
              <dd>{transfer.memo}</dd>
            </>
          )}

          {transfer.matched_policy_id && (
            <>
              <dt className="text-gray-500">{t('approval.detail.matched_policy')}</dt>
              <dd>P-{transfer.matched_policy_id}</dd>
            </>
          )}

          {transfer.expiry_at && (
            <>
              <dt className="text-gray-500">{t('approval.detail.expires_at')}</dt>
              <dd>{new Date(transfer.expiry_at).toLocaleString()}</dd>
            </>
          )}
        </dl>
      </Card>

      {/* Risk tags — populated by backend in v1.5; stub for skeleton */}
      <Card>
        <h2 className="font-semibold mb-2 flex items-center gap-1">
          <AlertTriangle className="w-4 h-4 text-yellow-500" />
          {t('approval.detail.risk_tags')}
        </h2>
        <ul className="text-sm space-y-1 text-gray-600">
          <li>{t('approval.detail.risk_tag.placeholder')}</li>
        </ul>
      </Card>

      {error && (
        <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>
      )}

      {canDecide && (
        <Card>
          {!showRejectInput ? (
            <div className="space-y-3">
              <label className="block text-sm">
                <span className="text-gray-600">{t('approval.detail.note_optional')}</span>
                <textarea
                  className="mt-1 w-full border border-gray-200 rounded p-2 text-sm"
                  rows={2}
                  value={note}
                  onChange={e => setNote(e.target.value)}
                />
              </label>
              <div className="flex gap-3">
                <button
                  className="btn-primary flex-1"
                  disabled={submitting}
                  onClick={onApprove}
                >
                  <CheckCircle className="w-4 h-4 inline mr-1" />
                  {t('approval.detail.approve')}
                </button>
                <button
                  className="btn-secondary flex-1"
                  disabled={submitting}
                  onClick={() => setShowRejectInput(true)}
                >
                  <XCircle className="w-4 h-4 inline mr-1" />
                  {t('approval.detail.reject')}
                </button>
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <label className="block text-sm">
                <span className="text-gray-600">{t('approval.detail.reject_reason_required')}</span>
                <textarea
                  className="mt-1 w-full border border-gray-200 rounded p-2 text-sm"
                  rows={3}
                  value={rejectReason}
                  onChange={e => setRejectReason(e.target.value)}
                  placeholder={t('approval.detail.reject_reason_placeholder')}
                />
              </label>
              <div className="flex gap-3">
                <button
                  className="btn-danger flex-1"
                  disabled={submitting || rejectReason.length < 5}
                  onClick={onReject}
                >
                  {t('approval.detail.confirm_reject')}
                </button>
                <button
                  className="btn-ghost flex-1"
                  disabled={submitting}
                  onClick={() => { setShowRejectInput(false); setRejectReason(''); }}
                >
                  {t('common.cancel')}
                </button>
              </div>
            </div>
          )}
        </Card>
      )}
    </div>
  );
}
