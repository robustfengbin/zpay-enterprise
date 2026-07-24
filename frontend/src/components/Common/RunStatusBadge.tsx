import React from 'react';

/**
 * Colored status badge for F4 run lifecycles (migration + batch transfer
 * share the same status union). Label comes in already translated so the
 * component stays namespace-agnostic (migration.run_status.* vs
 * batch.run_status.*).
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

const PALETTE: Record<RunLifecycleStatus, string> = {
  pending: 'bg-gray-100 text-gray-700',
  awaiting_approval: 'bg-amber-100 text-amber-800',
  approved: 'bg-blue-100 text-blue-800',
  rejected: 'bg-red-100 text-red-800',
  executing: 'bg-blue-100 text-blue-800',
  partial: 'bg-amber-100 text-amber-800',
  completed: 'bg-green-100 text-green-800',
  failed: 'bg-red-100 text-red-800',
  canceled: 'bg-gray-200 text-gray-600',
};

export function RunStatusBadge({
  status,
  label,
}: {
  status: RunLifecycleStatus | string;
  label: string;
}) {
  const cls = PALETTE[status as RunLifecycleStatus] ?? 'bg-gray-100 text-gray-700';
  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 rounded-full text-xs font-medium ${cls}`}
    >
      {status === 'executing' && (
        <span className="w-1.5 h-1.5 rounded-full bg-blue-600 mr-1.5 animate-pulse" />
      )}
      {label}
    </span>
  );
}
