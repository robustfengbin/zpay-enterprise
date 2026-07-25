import React from 'react';
import { Check } from 'lucide-react';

/**
 * Wizard progress. Completed steps are clickable so a reviewer can step back
 * without losing what they already filled in.
 */
export function Stepper({
  steps,
  current,
  onStepClick,
}: {
  steps: string[];
  /** 1-based index of the active step. */
  current: number;
  onStepClick?: (step: number) => void;
}) {
  return (
    <ol className="flex items-center gap-2" aria-label="progress">
      {steps.map((label, i) => {
        const n = i + 1;
        const done = n < current;
        const active = n === current;
        const clickable = done && !!onStepClick;
        return (
          <React.Fragment key={label}>
            {i > 0 && (
              <li
                aria-hidden="true"
                className={`h-px flex-1 ${n <= current ? 'bg-brand-200' : 'bg-line-200'}`}
              />
            )}
            <li>
              <button
                type="button"
                disabled={!clickable}
                onClick={() => clickable && onStepClick(n)}
                className={`flex items-center gap-2 rounded-full py-1 pl-1 pr-3 transition-colors ${
                  clickable ? 'hover:bg-line-100 cursor-pointer' : 'cursor-default'
                }`}
                aria-current={active ? 'step' : undefined}
              >
                <span
                  className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-[0.6875rem] font-semibold transition-colors ${
                    active
                      ? 'bg-brand-600 text-white shadow-[0_0_0_3px_var(--color-brand-100)]'
                      : done
                        ? 'bg-brand-50 text-brand-700 border border-brand-200'
                        : 'bg-line-100 text-ink-300 border border-line-200'
                  }`}
                >
                  {done ? <Check className="h-3.5 w-3.5" /> : n}
                </span>
                <span
                  className={`text-[0.8125rem] ${
                    active ? 'font-medium text-ink-900' : 'text-ink-400'
                  }`}
                >
                  {label}
                </span>
              </button>
            </li>
          </React.Fragment>
        );
      })}
    </ol>
  );
}
