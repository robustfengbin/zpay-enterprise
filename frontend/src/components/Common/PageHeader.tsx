import React, { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { ChevronLeft } from 'lucide-react';

interface PageHeaderProps {
  title: ReactNode;
  subtitle?: ReactNode;
  /** Optional "back to list" affordance rendered above the title. */
  backTo?: { to: string; label: string };
  /** Right-aligned actions (buttons, refresh, status). */
  actions?: ReactNode;
  /** Rendered under the subtitle — status badges, meta chips. */
  meta?: ReactNode;
}

/**
 * One page-title pattern for every screen: back link, title, subtitle, meta
 * chips, and a right-hand action cluster that always sits on the title's
 * baseline. Pages stop inventing their own header spacing.
 */
export function PageHeader({ title, subtitle, backTo, actions, meta }: PageHeaderProps) {
  return (
    <header className="mb-5">
      {backTo && (
        <Link
          to={backTo.to}
          className="inline-flex items-center gap-1 text-xs text-ink-400 hover:text-ink-700 transition-colors mb-2"
        >
          <ChevronLeft className="w-3.5 h-3.5" />
          {backTo.label}
        </Link>
      )}
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <h1 className="text-[1.375rem] leading-tight font-semibold text-ink-900 tracking-[-0.01em]">
            {title}
          </h1>
          {subtitle && <p className="mt-1 text-[0.8125rem] text-ink-400">{subtitle}</p>}
          {meta && <div className="mt-2.5 flex flex-wrap items-center gap-2">{meta}</div>}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
      </div>
    </header>
  );
}
