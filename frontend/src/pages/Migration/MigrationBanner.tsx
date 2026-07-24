import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { ArrowRight, Shield } from 'lucide-react';
import { migrationService, zatToZec } from '../../services/api/migration';
import type { MigrationStatus } from '../../types/migration';

/**
 * F4.1 — wallet-page banner: shown when a Zcash wallet still holds shielded
 * funds in the legacy Orchard pool (or has a migration run in flight).
 * Renders nothing while loading or when there is nothing to migrate, so it
 * can be dropped into the wallet page unconditionally.
 */
export function MigrationBanner({ walletId }: { walletId: number }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [status, setStatus] = useState<MigrationStatus | null>(null);

  useEffect(() => {
    let alive = true;
    migrationService
      .walletStatus(walletId)
      .then(s => { if (alive) setStatus(s); })
      // Banner is advisory — a failed status fetch must not break the page.
      .catch(() => { if (alive) setStatus(null); });
    return () => { alive = false; };
  }, [walletId]);

  if (!status) return null;
  const hasFunds = status.spendable_zatoshis > 0;
  const hasActive = status.active_run_id !== null;
  if (!hasFunds && !hasActive) return null;

  return (
    <div className="rounded-lg border border-blue-200 bg-blue-50 px-4 py-3 flex items-center justify-between gap-4">
      <div className="flex items-start gap-3">
        <Shield className="w-5 h-5 text-blue-600 mt-0.5 shrink-0" />
        <div className="text-sm">
          {hasActive ? (
            <p className="text-blue-900">
              {t('migration.banner.active', { id: status.active_run_id, status: t(`migration.run_status.${status.active_run_status}`) })}
            </p>
          ) : (
            <>
              <p className="font-semibold text-blue-900">{t('migration.banner.title')}</p>
              <p className="text-blue-800">
                {t('migration.banner.body', {
                  amount: zatToZec(status.spendable_zatoshis),
                  notes: status.unspent_note_count,
                })}
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
        <ArrowRight className="w-4 h-4 inline ml-1" />
      </button>
    </div>
  );
}
