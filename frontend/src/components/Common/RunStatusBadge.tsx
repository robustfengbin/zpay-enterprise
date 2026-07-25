import React from 'react';

/**
 * Status badge for F4 run lifecycles (migration + batch transfer share the
 * same status union) and for the per-batch item statuses. Labels arrive
 * already translated so the component stays namespace-agnostic
 * (migration.run_status.* vs batch.run_status.*).
 */
export type RunLifecycleStatus =
  | 'pending'
  | 'awaiting_approval'
  | 'approved'
  | 'rejected'
  | 'executing'
  | 'partial'
  | 'completed'
  | 'failed'
  | 'canceled';

/** Tone per state — brand tones are reserved for "work in progress". */
const TONE: Record<RunLifecycleStatus, string> = {
  pending: 'badge-neutral',
  awaiting_approval: 'badge-warn',
  approved: 'badge-brand',
  rejected: 'badge-bad',
  executing: 'badge-brand',
  partial: 'badge-warn',
  completed: 'badge-ok',
  failed: 'badge-bad',
  canceled: 'badge-neutral',
};

/** States that are still moving on their own get a breathing dot. */
const LIVE = new Set(['executing', 'approved']);

export function RunStatusBadge({
  status,
  label,
}: {
  status: RunLifecycleStatus | string;
  label: string;
}) {
  const tone = TONE[status as RunLifecycleStatus] ?? 'badge-neutral';
  return (
    <span className={`badge ${tone}`}>
      {LIVE.has(status) && (
        <span className="dot-live h-1.5 w-1.5 rounded-full bg-brand-500" aria-hidden="true" />
      )}
      {label}
    </span>
  );
}

const ITEM_TONE: Record<string, string> = {
  pending: 'badge-neutral',
  submitted: 'badge-ok',
  failed: 'badge-bad',
  canceled: 'badge-neutral',
};

/** Per-batch status pill used inside run detail tables. */
export function ItemStatusBadge({ status, label }: { status: string; label: string }) {
  return <span className={`badge ${ITEM_TONE[status] ?? 'badge-neutral'}`}>{label}</span>;
}
