import React, { ReactNode } from 'react';
import type { LucideIcon } from 'lucide-react';

/**
 * Empty states offer the next action instead of announcing "no data".
 */
export function EmptyState({
  icon: Icon,
  title,
  body,
  action,
}: {
  icon: LucideIcon;
  title: string;
  body?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center px-6 py-14 text-center">
      <div className="flex h-11 w-11 items-center justify-center rounded-xl border border-line-200 bg-surface-2">
        <Icon className="h-5 w-5 text-ink-300" />
      </div>
      <p className="mt-3.5 text-sm font-medium text-ink-900">{title}</p>
      {body && <p className="mt-1 max-w-sm text-[0.8125rem] leading-relaxed text-ink-400">{body}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}
