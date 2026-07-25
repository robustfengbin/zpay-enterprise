import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ArrowRight, Lock } from 'lucide-react';
import { Amount } from '../../components/Common';
import { migrationService, zatToZec } from '../../services/api/migration';
import type { MigrationStatus } from '../../types/migration';

/**
 * F4.1 — wallet-page banner: shown when a Zcash wallet still holds shielded
 * funds in the legacy Orchard pool (or has a migration run in flight).
 * Renders nothing while loading or when there is nothing to migrate, so it
 * can be dropped into the wallet page unconditionally.
 */
export function MigrationBanner({
  walletId,
  walletName,
}: {
  walletId: number;
  /** Shown in the banner so a multi-wallet page says which wallet it means. */
  walletName?: string;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [status, setStatus] = useState<MigrationStatus | null>(null);

  useEffect(() => {
    let alive = true;
    migrationService
      .walletStatus(walletId)
      .then((s) => {
        if (alive) setStatus(s);
      })
      // Banner is advisory — a failed status fetch must not break the page.
      .catch(() => {
        if (alive) setStatus(null);
      });
    return () => {
      alive = false;
    };
  }, [walletId]);

  if (!status) return null;
  const hasFunds = status.spendable_zatoshis > 0;
  const hasActive = status.active_run_id !== null;
  if (!hasFunds && !hasActive) return null;

  return (
    <div className="flex flex-wrap items-center justify-between gap-4 rounded-[10px] border border-legacy-200 bg-legacy-50 px-4 py-3.5">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg border border-legacy-200 bg-white">
          <Lock className="h-3.5 w-3.5 text-legacy-600" />
        </span>
        <div className="min-w-0">
          {hasActive ? (
            <p className="text-[0.8125rem] text-legacy-700">
              {walletName && <span className="font-semibold">{walletName} · </span>}
              {t('migration.banner.active', {
                id: status.active_run_id,
                status: t(`migration.run_status.${status.active_run_status}`),
              })}
            </p>
          ) : (
            <>
              <p className="text-[0.8125rem] font-semibold text-legacy-700">
                {walletName && <span className="text-legacy-600">{walletName} · </span>}
                {t('migration.banner.title')}
              </p>
              <p className="mt-0.5 text-[0.8125rem] text-legacy-700/85">
                <Amount value={zatToZec(status.spendable_zatoshis)} />
                <span className="mx-1.5 text-legacy-500">·</span>
                {t('migration.banner.body_notes', { notes: status.unspent_note_count })}
              </p>
            </>
          )}
        </div>
      </div>
      <button
        className="btn-primary whitespace-nowrap"
        onClick={() =>
          hasActive
            ? navigate(`/migrations/${status.active_run_id}`)
            : navigate(`/migrations/new?wallet=${walletId}`)
        }
      >
        {hasActive ? t('migration.banner.view') : t('migration.banner.start')}
        <ArrowRight className="h-4 w-4" />
      </button>
    </div>
  );
}
