import React, { ReactNode, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Copy } from 'lucide-react';

/**
 * Small display primitives for financial data. Amounts line up, hashes are
 * copyable, timestamps read as "2 hours ago" but keep the exact value one
 * hover away — the three things a treasury operator does all day.
 */

/** Money. Tabular figures, unit de-emphasised, trailing zeros trimmed. */
export function Amount({
  value,
  unit = 'ZEC',
  className = '',
  strong,
}: {
  value: string | number;
  unit?: string | null;
  className?: string;
  strong?: boolean;
}) {
  const n = Number(value);
  let text = Number.isFinite(n) ? trimZeros(n.toFixed(8)) : String(value);
  // Never round a non-zero amount away to "0" — show the raw value instead.
  if (text === '0' && n !== 0) text = trimZeros(String(value));
  return (
    <span className={`num whitespace-nowrap ${className}`}>
      <span className={strong ? 'font-semibold text-ink-900' : ''}>{text}</span>
      {unit && <span className="ml-1 text-ink-400 text-[0.9em]">{unit}</span>}
    </span>
  );
}

function trimZeros(s: string): string {
  return s.includes('.') ? s.replace(/\.?0+$/, '') : s;
}

/** Transaction id / address: middle-truncated, monospace, click to copy. */
export function Hash({
  value,
  head = 10,
  tail = 6,
  className = '',
}: {
  value: string | null | undefined;
  head?: number;
  tail?: number;
  className?: string;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  if (!value) return <span className="text-ink-300">—</span>;

  const short =
    value.length > head + tail + 2 ? `${value.slice(0, head)}…${value.slice(-tail)}` : value;

  async function copy() {
    try {
      await navigator.clipboard.writeText(value!);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      /* clipboard blocked (insecure origin) — the title attribute still shows the full value */
    }
  }

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        void copy();
      }}
      title={`${value}\n${t('common.copy')}`}
      className={`group inline-flex items-center gap-1.5 mono text-[0.75rem] text-ink-500 hover:text-ink-900 transition-colors ${className}`}
    >
      <span>{short}</span>
      {copied ? (
        <Check className="w-3 h-3 text-ok-600" />
      ) : (
        <Copy className="w-3 h-3 opacity-0 group-hover:opacity-100 transition-opacity" />
      )}
    </button>
  );
}

/** Relative time with the exact timestamp on hover. */
export function TimeAgo({ value, className = '' }: { value: string | null; className?: string }) {
  const { i18n } = useTranslation();
  if (!value) return <span className="text-ink-300">—</span>;

  const date = new Date(value);
  const diffMs = date.getTime() - Date.now();
  const abs = Math.abs(diffMs);
  const rtf = new Intl.RelativeTimeFormat(i18n.language, { numeric: 'auto' });

  let text: string;
  if (abs < 60_000) text = rtf.format(Math.round(diffMs / 1000), 'second');
  else if (abs < 3_600_000) text = rtf.format(Math.round(diffMs / 60_000), 'minute');
  else if (abs < 86_400_000) text = rtf.format(Math.round(diffMs / 3_600_000), 'hour');
  else text = rtf.format(Math.round(diffMs / 86_400_000), 'day');

  return (
    <span className={`whitespace-nowrap ${className}`} title={date.toLocaleString()}>
      {text}
    </span>
  );
}

/** Label / value pair used in the summary strips on detail pages. */
export function Stat({
  label,
  children,
  hint,
}: {
  label: ReactNode;
  children: ReactNode;
  hint?: ReactNode;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-[0.6875rem] font-medium uppercase tracking-[0.04em] text-ink-400">
        {label}
      </dt>
      <dd className="mt-1 text-sm text-ink-900">{children}</dd>
      {hint && <p className="mt-0.5 text-[0.6875rem] text-ink-400">{hint}</p>}
    </div>
  );
}

/** Row of stats, hairline-separated on wide screens. */
export function StatRow({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <dl className={`grid gap-x-6 gap-y-4 sm:grid-cols-2 lg:grid-cols-4 ${className}`}>{children}</dl>
  );
}
