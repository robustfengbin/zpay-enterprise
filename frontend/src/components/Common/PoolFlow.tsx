import React from 'react';
import { useTranslation } from 'react-i18next';
import { ArrowRight, Lock, ShieldCheck } from 'lucide-react';
import { Amount } from './DataBits';

interface PoolFlowProps {
  /** Amount still sitting in the legacy Orchard pool. */
  legacy: string | number;
  /** Amount that has landed in Ironwood. */
  ironwood: string | number;
  /** 0–100. Share of the run's planned total already through the turnstile. */
  pct: number;
  /** Sub-line under the bar (batch counts, schedule). */
  caption?: string;
  /** Animate the arrow while a run is actually moving funds. */
  live?: boolean;
}

/**
 * The release's core mental model in one object: funds leave a pool that is
 * closed to deposits and arrive in Ironwood, one turnstile batch at a time.
 *
 * The two amounts are passed in rather than derived here, so the same layout
 * serves both today's run-level arithmetic and the per-pool wallet balances
 * the dual-pool scanner will publish.
 */
export function PoolFlow({ legacy, ironwood, pct, caption, live }: PoolFlowProps) {
  const { t } = useTranslation();
  const clamped = Math.max(0, Math.min(100, Math.round(pct)));

  return (
    <div className="card overflow-hidden">
      {/* auto | 1fr | auto — the tiles hug their content and a dashed rail
          carries the eye across whatever width is left between them. */}
      <div className="grid grid-cols-[auto_minmax(2rem,1fr)_auto] items-center gap-2 px-4 py-4">
        <PoolSide
          tone="legacy"
          icon={<Lock className="h-3.5 w-3.5" />}
          label={t('migration.pool.legacy')}
          sub={t('migration.pool.legacy_sub')}
          amount={legacy}
        />

        <div className="flex flex-col items-center gap-1.5">
          <span className="flex w-full items-center gap-1.5">
            <span className="h-px flex-1 border-t border-dashed border-line-300" />
            <ArrowRight
              className={`h-3.5 w-3.5 shrink-0 text-ink-300 ${live ? 'dot-live' : ''}`}
              aria-hidden="true"
            />
            <span className="h-px flex-1 border-t border-dashed border-line-300" />
          </span>
          <span className="text-[0.625rem] font-medium uppercase tracking-[0.06em] text-ink-400">
            {t('migration.pool.turnstile')}
          </span>
        </div>

        <PoolSide
          tone="iron"
          icon={<ShieldCheck className="h-3.5 w-3.5" />}
          label={t('migration.pool.ironwood')}
          sub={t('migration.pool.ironwood_sub')}
          amount={ironwood}
          align="right"
        />
      </div>

      <div className="border-t border-line-200 px-4 py-3">
        <div className="progress progress-iron">
          <span style={{ width: `${clamped}%` }} />
        </div>
        <div className="mt-2 flex items-center justify-between text-[0.6875rem] text-ink-400">
          <span>{caption}</span>
          <span className="num font-medium text-ink-700">{clamped}%</span>
        </div>
      </div>
    </div>
  );
}

function PoolSide({
  tone,
  icon,
  label,
  sub,
  amount,
  align = 'left',
}: {
  tone: 'legacy' | 'iron';
  icon: React.ReactNode;
  label: string;
  sub: string;
  amount: string | number;
  align?: 'left' | 'right';
}) {
  const toneCls =
    tone === 'legacy'
      ? 'text-legacy-700 bg-legacy-50 border-legacy-200'
      : 'text-iron-700 bg-iron-50 border-iron-200';
  return (
    <div className={align === 'right' ? 'text-right' : ''}>
      <div
        className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[0.6875rem] font-medium ${toneCls}`}
      >
        {icon}
        {label}
      </div>
      <div className="mt-2 text-lg leading-none tracking-[-0.01em]">
        <Amount value={amount} strong />
      </div>
      <p className="mt-1.5 text-[0.6875rem] text-ink-400">{sub}</p>
    </div>
  );
}
