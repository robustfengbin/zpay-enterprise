import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { ArrowLeft, ArrowRight, Shield, Zap } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
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
        setWallets(all.filter(w => w.chain === 'zcash'));
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  useEffect(() => {
    if (walletId === '') { setStatus(null); return; }
    migrationService
      .walletStatus(Number(walletId))
      .then(setStatus)
      .catch(e => setError((e as Error).message));
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

  return (
    <div className="p-6 max-w-3xl space-y-4">
      <header>
        <h1 className="text-2xl font-semibold">{t('migration.create.title')}</h1>
        <p className="text-sm text-gray-500">{t('migration.create.subtitle')}</p>
      </header>

      {/* Step indicator */}
      <div className="flex items-center py-2">
        {[1, 2, 3].map((s, i) => (
          <React.Fragment key={s}>
            {i > 0 && (
              <div className={`h-px flex-1 mx-3 ${s <= step ? 'bg-blue-500' : 'bg-gray-200'}`} />
            )}
            <div className="flex items-center gap-2">
              <span
                className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-semibold ${
                  s === step
                    ? 'bg-blue-600 text-white ring-4 ring-blue-100'
                    : s < step
                    ? 'bg-blue-100 text-blue-700'
                    : 'bg-gray-100 text-gray-400'
                }`}
              >
                {s < step ? '✓' : s}
              </span>
              <span
                className={`text-sm ${
                  s === step ? 'font-semibold text-gray-900' : 'text-gray-500'
                }`}
              >
                {t(`migration.create.step${s}`)}
              </span>
            </div>
          </React.Fragment>
        ))}
      </div>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      {step === 1 && (
        <Card>
          <h2 className="font-semibold mb-2">{t('migration.create.what_title')}</h2>
          <p className="text-sm text-gray-700 whitespace-pre-line mb-4">{t('migration.create.what_body')}</p>

          <label className="block text-sm text-gray-600 mb-1">{t('migration.create.wallet')}</label>
          <select
            className="input w-full mb-2"
            value={walletId}
            onChange={e => setWalletId(e.target.value === '' ? '' : Number(e.target.value))}
          >
            <option value="">{t('migration.create.select_wallet')}</option>
            {wallets.map(w => (
              <option key={w.id} value={w.id}>
                #{w.id} {w.name}
              </option>
            ))}
          </select>

          {status && (
            <dl className="grid grid-cols-2 gap-y-1 text-sm mt-2">
              <dt className="text-gray-500">{t('migration.create.spendable')}</dt>
              <dd className="font-mono">{spendable} ZEC</dd>
              <dt className="text-gray-500">{t('migration.create.note_count')}</dt>
              <dd>{status.unspent_note_count}</dd>
            </dl>
          )}
          {hasActive && (
            <p className="text-sm text-yellow-800 bg-yellow-50 rounded px-3 py-2 mt-2">
              {t('migration.create.active_exists', { id: status?.active_run_id })}
            </p>
          )}

          <div className="mt-4 flex justify-end">
            <button
              className="btn-primary"
              disabled={walletId === '' || hasActive || !status || status.spendable_zatoshis === 0}
              onClick={() => setStep(2)}
            >
              {t('common.next')} <ArrowRight className="w-4 h-4 inline ml-1" />
            </button>
          </div>
        </Card>
      )}

      {step === 2 && (
        <Card>
          <h2 className="font-semibold mb-3">{t('migration.create.mode_title')}</h2>
          <div className="grid grid-cols-2 gap-3">
            <button
              className={`text-left border rounded-lg p-4 ${mode === 'private' ? 'border-blue-600 ring-1 ring-blue-600' : 'border-gray-200'}`}
              onClick={() => setMode('private')}
            >
              <div className="flex items-center gap-2 font-semibold mb-1">
                <Shield className="w-4 h-4 text-blue-600" /> {t('migration.create.mode_private')}
              </div>
              <p className="text-xs text-gray-600">{t('migration.create.mode_private_desc')}</p>
              <p className="text-xs text-gray-500 mt-2">{t('migration.create.mode_private_fee')}</p>
            </button>
            <button
              className={`text-left border rounded-lg p-4 ${mode === 'immediate' ? 'border-blue-600 ring-1 ring-blue-600' : 'border-gray-200'}`}
              onClick={() => setMode('immediate')}
            >
              <div className="flex items-center gap-2 font-semibold mb-1">
                <Zap className="w-4 h-4 text-amber-600" /> {t('migration.create.mode_immediate')}
              </div>
              <p className="text-xs text-gray-600">{t('migration.create.mode_immediate_desc')}</p>
              <p className="text-xs text-gray-500 mt-2">{t('migration.create.mode_immediate_fee')}</p>
            </button>
          </div>

          {mode === 'private' && (
            <div className="grid grid-cols-2 gap-3 mt-4">
              <div>
                <label className="block text-sm text-gray-600 mb-1">{t('migration.create.batches')}</label>
                <input
                  type="number"
                  min={2}
                  max={50}
                  className="input w-full"
                  value={batchCount}
                  onChange={e => setBatchCount(Math.max(2, Math.min(50, Number(e.target.value) || 2)))}
                />
              </div>
              <div>
                <label className="block text-sm text-gray-600 mb-1">{t('migration.create.window')}</label>
                <input
                  type="number"
                  min={1}
                  max={336}
                  className="input w-full"
                  value={windowHours}
                  onChange={e => setWindowHours(Math.max(1, Math.min(336, Number(e.target.value) || 1)))}
                />
              </div>
            </div>
          )}

          <p className="text-xs text-gray-500 mt-3">{t('migration.create.privacy_disclaimer')}</p>

          <div className="mt-4 flex justify-between">
            <button className="btn-ghost" onClick={() => setStep(1)}>
              <ArrowLeft className="w-4 h-4 inline mr-1" /> {t('common.back')}
            </button>
            <button className="btn-primary" onClick={() => setStep(3)}>
              {t('common.next')} <ArrowRight className="w-4 h-4 inline ml-1" />
            </button>
          </div>
        </Card>
      )}

      {step === 3 && (
        <Card>
          <h2 className="font-semibold mb-3">{t('migration.create.confirm_title')}</h2>
          <dl className="grid grid-cols-2 gap-y-2 text-sm">
            <dt className="text-gray-500">{t('migration.create.wallet')}</dt>
            <dd>#{walletId}</dd>
            <dt className="text-gray-500">{t('migration.create.spendable')}</dt>
            <dd className="font-mono">{spendable} ZEC</dd>
            <dt className="text-gray-500">{t('migration.create.mode')}</dt>
            <dd>{t(`migration.create.mode_${mode}`)}</dd>
            {mode === 'private' && (
              <>
                <dt className="text-gray-500">{t('migration.create.batches')}</dt>
                <dd>{batchCount}</dd>
                <dt className="text-gray-500">{t('migration.create.window')}</dt>
                <dd>{windowHours} h</dd>
              </>
            )}
          </dl>

          <label className="block text-sm text-gray-600 mt-3 mb-1">{t('migration.create.notes')}</label>
          <input
            className="input w-full"
            value={notes}
            maxLength={500}
            onChange={e => setNotes(e.target.value)}
            placeholder={t('migration.create.notes_placeholder')}
          />

          <p className="text-xs text-gray-500 mt-3">{t('migration.create.confirm_hint')}</p>

          <div className="mt-4 flex justify-between">
            <button className="btn-ghost" onClick={() => setStep(2)}>
              <ArrowLeft className="w-4 h-4 inline mr-1" /> {t('common.back')}
            </button>
            <button className="btn-primary" disabled={busy} onClick={onCreate}>
              {t('migration.create.submit')}
            </button>
          </div>
        </Card>
      )}
    </div>
  );
}
