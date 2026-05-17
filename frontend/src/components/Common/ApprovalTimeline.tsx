import React from 'react';
import { useTranslation } from 'react-i18next';
import type { TransferApproval } from '../../types/approval';

interface TimelineEvent {
  ts: string;
  actor: string;
  kind: 'create' | 'approve' | 'reject' | 'expire' | 'execute' | 'confirm' | 'fail';
  detail?: string;
}

interface ApprovalTimelineProps {
  events: TimelineEvent[];
}

const dotByKind: Record<TimelineEvent['kind'], string> = {
  create:  'bg-blue-400',
  approve: 'bg-green-500',
  reject:  'bg-red-500',
  expire:  'bg-gray-400',
  execute: 'bg-blue-500',
  confirm: 'bg-green-600',
  fail:    'bg-red-500',
};

/**
 * Vertical timeline used by F2.1 §6 Page 5 (history detail) +
 * F3.1 run history. Renders an ordered series of approval/transfer events.
 */
export function ApprovalTimeline({ events }: ApprovalTimelineProps) {
  const { t } = useTranslation();
  return (
    <ol className="relative border-l-2 border-gray-200 ml-2 py-2 space-y-3">
      {events.map((evt, idx) => (
        <li key={idx} className="ml-4">
          <span
            className={`absolute -left-[7px] w-3 h-3 rounded-full ${dotByKind[evt.kind]}`}
            aria-hidden
          />
          <time className="text-xs text-gray-500">{new Date(evt.ts).toLocaleString()}</time>
          <p className="text-sm">
            <span className="font-medium">{evt.actor}</span>
            {' — '}
            {t(`approval.event.${evt.kind}`)}
            {evt.detail && <span className="text-gray-600"> · {evt.detail}</span>}
          </p>
        </li>
      ))}
    </ol>
  );
}

/**
 * Helper to convert raw TransferApproval[] + create event into TimelineEvent[].
 */
export function buildTimelineFromApprovals(
  approvals: TransferApproval[],
  createdBy: string,
  createdAt: string,
  txEvents: { ts: string; kind: 'execute' | 'confirm' | 'fail'; detail?: string }[] = [],
): TimelineEvent[] {
  const events: TimelineEvent[] = [
    { ts: createdAt, actor: createdBy, kind: 'create' },
    ...approvals.map<TimelineEvent>(a => ({
      ts: a.created_at,
      actor: a.approver_username,
      kind: a.decision === 'approve' ? 'approve' : 'reject',
      detail: a.reason || undefined,
    })),
    ...txEvents.map<TimelineEvent>(e => ({
      ts: e.ts,
      actor: 'system',
      kind: e.kind,
      detail: e.detail,
    })),
  ];
  // Sort by timestamp ascending
  return events.sort((a, b) => new Date(a.ts).getTime() - new Date(b.ts).getTime());
}
