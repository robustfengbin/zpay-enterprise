import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { ArrowLeft, ArrowRight, Coins, Info, Lock, ShieldCheck, Zap } from 'lucide-react';
import { Amount, Card, LoadingSpinner, PageHeader, Stepper } from '../../components/Common';
import { migrationService, zatToZec } from '../../services/api/migration';
import { walletService } from '../../services/api';
import type { MigrationMode, MigrationStatus } from '../../types/migration';

interface ZcashWalletOption {
  id: number;
  name: string;
  address: string;
}

/**
 * F4.1 — Migration wizard (PRD-F4 §7): ① what & why → ② mode choice with
 * the privacy/fee trade-off side by side → ③ confirm (batch plan preview,
 * approval hint). Addresses do not change; the plan totals what is
 * spendable at creation time.
 */
export function MigrationRunCreate() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const preselected = params.get('wallet');

  const [step, setStep] = useState(1);
  const [wallets, setWallets] = useState<ZcashWalletOption[]>([]);
  const [walletId, setWalletId] = useState<number | ''>(preselected ? Number(preselected) : '');
  const [status, setStatus] = useState<MigrationStatus | null>(null);
  const [mode, setMode] = useState<MigrationMode>('private');
  const [batchCount, setBatchCount] = useState(6);
  const [windowHours, setWindowHours] = useState(48);
  const [notes, setNotes] = useState('');
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const all = (await walletService.listWallets('zcash')) as Array<
          ZcashWalletOption & { chain: string }
        >;
        setWallets(all.filter((w) => w.chain === 'zcash'));
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  useEffect(() => {
    if (walletId === '') {
      setStatus(null);
      return;
    }
    migrationService
      .walletStatus(Number(walletId))
      .then(setStatus)
      .catch((e) => setError((e as Error).message));
  }, [walletId]);

  async function onCreate() {
    if (walletId === '') return;
    setBusy(true);
    setError(null);
    try {
      const summary = await migrationService.createRun({
        source_wallet_id: Number(walletId),
        mode,
        batch_count: mode === 'private' ? batchCount : undefined,
        window_hours: mode === 'private' ? windowHours : undefined,
        notes: notes.trim() || undefined,
      });
      navigate(`/migrations/${summary.run.id}`);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }

  if (loading) return <LoadingSpinner />;

  const spendable = status ? zatToZec(status.spendable_zatoshis) : null;
  const hasActive = !!status?.active_run_id;
  const nothingToMigrate = !!status && status.spendable_zatoshis === 0;
  const blockedReason = hasActive
    ? t('migration.create.active_exists', { id: status?.active_run_id })
    : nothingToMigrate
      ? t('migration.create.nothing_to_migrate')
      : null;
  const selectedWallet = wallets.find((w) => w.id === Number(walletId));
  const everyHours = mode === 'private' ? windowHours / Math.max(1, batchCount) : 0;

  return (
    <div className="max-w-3xl">
      <PageHeader
        backTo={{ to: '/migrations', label: t('migration.detail.back_to_list') }}
        title={t('migration.create.title')}
        subtitle={t('migration.create.subtitle')}
      />

      <div className="mb-5">
        <Stepper
          steps={[t('migration.create.step1'), t('migration.create.step2'), t('migration.create.step3')]}
          current={step}
          onStepClick={setStep}
        />
      </div>

      {error && <div className="alert alert-bad mb-4">{error}</div>}

      {step === 1 && (
        <Card title={t('migration.create.what_title')}>
          <p className="whitespace-pre-line text-[0.8125rem] leading-relaxed text-ink-500">
            {t('migration.create.what_body')}
          </p>

          <div className="mt-5">
            <label className="label" htmlFor="wallet-select">
              {t('migration.create.wallet')}
            </label>
            <select
              id="wallet-select"
              className="field"
              value={walletId}
              onChange={(e) => setWalletId(e.target.value === '' ? '' : Number(e.target.value))}
            >
              <option value="">{t('migration.create.select_wallet')}</option>
              {wallets.map((w) => (
                <option key={w.id} value={w.id}>
                  #{w.id} {w.name}
                </option>
              ))}
            </select>
          </div>

          {status && (
            <div className="mt-4 rounded-[10px] border border-legacy-200 bg-legacy-50 px-4 py-3.5">
              <div className="flex items-center gap-1.5 text-[0.6875rem] font-medium text-legacy-700">
                <Lock className="h-3.5 w-3.5" />
                {t('migration.pool.legacy')}
              </div>
              <div className="mt-2 flex flex-wrap items-baseline gap-x-5 gap-y-1">
                <span className="text-xl leading-none">
                  <Amount value={spendable ?? 0} strong />
                </span>
                <span className="text-[0.75rem] text-legacy-700/80">
                  {t('migration.create.note_count')}: {status.unspent_note_count}
                </span>
              </div>
            </div>
          )}

          {blockedReason && <div className="alert alert-warn mt-3">{blockedReason}</div>}

          <div className="mt-5 flex justify-end">
            <button
              className="btn-primary"
              disabled={walletId === '' || hasActive || !status || nothingToMigrate}
              title={blockedReason ?? undefined}
              onClick={() => setStep(2)}
            >
              {t('common.next')} <ArrowRight className="h-4 w-4" />
            </button>
          </div>
        </Card>
      )}

      {step === 2 && (
        <Card title={t('migration.create.mode_title')}>
          <div className="grid gap-3 sm:grid-cols-2">
            <ModeCard
              selected={mode === 'private'}
              onSelect={() => setMode('private')}
              icon={<ShieldCheck className="h-4 w-4 text-brand-600" />}
              title={t('migration.create.mode_private')}
              desc={t('migration.create.mode_private_desc')}
              fee={t('migration.create.mode_private_fee')}
            />
            <ModeCard
              selected={mode === 'immediate'}
              onSelect={() => setMode('immediate')}
              icon={<Zap className="h-4 w-4 text-legacy-600" />}
              title={t('migration.create.mode_immediate')}
              desc={t('migration.create.mode_immediate_desc')}
              fee={t('migration.create.mode_immediate_fee')}
            />
          </div>

          {mode === 'private' && (
            <>
              <div className="mt-5 grid gap-4 sm:grid-cols-2">
                <div>
                  <label className="label" htmlFor="batches">
                    {t('migration.create.batches')}
                  </label>
                  <input
                    id="batches"
                    type="number"
                    min={2}
                    max={50}
                    className="field num"
                    value={batchCount}
                    onChange={(e) =>
                      setBatchCount(Math.max(2, Math.min(50, Number(e.target.value) || 2)))
                    }
                  />
                </div>
                <div>
                  <label className="label" htmlFor="window">
                    {t('migration.create.window')}
                  </label>
                  <input
                    id="window"
                    type="number"
                    min={1}
                    max={336}
                    className="field num"
                    value={windowHours}
                    onChange={(e) =>
                      setWindowHours(Math.max(1, Math.min(336, Number(e.target.value) || 1)))
                    }
                  />
                </div>
              </div>
              <p className="hint mt-3 flex items-start gap-1.5">
                <Info className="mt-px h-3.5 w-3.5 shrink-0" />
                {t('migration.create.plan_preview', {
                  batches: batchCount,
                  hours: windowHours,
                  every: everyHours >= 1 ? everyHours.toFixed(1) : (everyHours * 60).toFixed(0),
                  unit: everyHours >= 1 ? 'h' : 'min',
                })}
              </p>
            </>
          )}

          <p className="hint mt-4">{t('migration.create.privacy_disclaimer')}</p>

          <div className="mt-5 flex justify-between">
            <button className="btn-ghost" onClick={() => setStep(1)}>
              <ArrowLeft className="h-4 w-4" /> {t('common.back')}
            </button>
            <button className="btn-primary" onClick={() => setStep(3)}>
              {t('common.next')} <ArrowRight className="h-4 w-4" />
            </button>
          </div>
        </Card>
      )}

      {step === 3 && (
        <Card title={t('migration.create.confirm_title')}>
          <dl className="divide-y divide-line-100 text-[0.8125rem]">
            <Row label={t('migration.create.wallet')}>
              {selectedWallet ? `#${selectedWallet.id} ${selectedWallet.name}` : `#${walletId}`}
            </Row>
            <Row label={t('migration.create.spendable')}>
              <Amount value={spendable ?? 0} strong />
            </Row>
            <Row label={t('migration.create.mode')}>
              <span className={`badge ${mode === 'private' ? 'badge-brand' : 'badge-neutral'}`}>
                {mode === 'private' ? (
                  <ShieldCheck className="h-3 w-3" />
                ) : (
                  <Zap className="h-3 w-3" />
                )}
                {t(`migration.create.mode_${mode}`)}
              </span>
            </Row>
            {mode === 'private' && (
              <Row label={t('migration.create.batches')}>
                <span className="num">
                  {batchCount} × {windowHours}h
                </span>
              </Row>
            )}
          </dl>

          <div className="mt-4">
            <label className="label" htmlFor="notes">
              {t('migration.create.notes')}
            </label>
            <input
              id="notes"
              className="field"
              value={notes}
              maxLength={500}
              onChange={(e) => setNotes(e.target.value)}
              placeholder={t('migration.create.notes_placeholder')}
            />
          </div>

          <div className="alert alert-info mt-4">
            <Coins className="mt-px h-4 w-4 shrink-0" />
            <span>{t('migration.create.confirm_hint')}</span>
          </div>

          <div className="mt-5 flex justify-between">
            <button className="btn-ghost" onClick={() => setStep(2)}>
              <ArrowLeft className="h-4 w-4" /> {t('common.back')}
            </button>
            <button className="btn-primary" disabled={busy} onClick={onCreate}>
              {busy ? t('common.loading') : t('migration.create.submit')}
            </button>
          </div>
        </Card>
      )}
    </div>
  );
}

function ModeCard({
  selected,
  onSelect,
  icon,
  title,
  desc,
  fee,
}: {
  selected: boolean;
  onSelect: () => void;
  icon: React.ReactNode;
  title: string;
  desc: string;
  fee: string;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-pressed={selected}
      className={`rounded-[10px] border p-4 text-left transition-colors ${
        selected
          ? 'border-brand-400 bg-brand-50 shadow-[0_0_0_3px_var(--color-brand-100)]'
          : 'border-line-200 bg-surface hover:border-line-300 hover:bg-surface-2'
      }`}
    >
      <div className="flex items-center gap-2 text-[0.8125rem] font-semibold text-ink-900">
        {icon}
        {title}
      </div>
      <p className="mt-1.5 text-[0.75rem] leading-relaxed text-ink-500">{desc}</p>
      <p className="mt-2 text-[0.6875rem] text-ink-400">{fee}</p>
    </button>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-2.5">
      <dt className="text-ink-400">{label}</dt>
      <dd className="text-ink-900">{children}</dd>
    </div>
  );
}
