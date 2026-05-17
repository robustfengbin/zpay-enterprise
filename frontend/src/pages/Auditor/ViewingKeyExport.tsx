import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Copy, Download, Key, AlertTriangle } from 'lucide-react';
import { Card, Modal } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type { ViewingKeyExportResponse, ViewingKeyType } from '../../types/viewing-key';

interface ViewingKeyExportModalProps {
  walletId: number;
  walletName: string;
  open: boolean;
  onClose: () => void;
}

type Step = 'form' | 'token' | 'downloaded';

/**
 * F1.1 — Admin one-click export of an Orchard viewing key.
 *
 * Three-step flow (aligned with france's backend 7dbaa87):
 *   1. form       — pick key_type (OVK/IVK/UFVK) + re-verify admin password
 *   2. token      — backend issued an export_id + download_token (24h TTL).
 *                   Show the token + "claim download" CTA. Token is single-
 *                   use server-side; once claimed the row is zeroed.
 *   3. downloaded — show the actual key string with copy-to-clipboard and
 *                   a strong reminder that this is a one-time view.
 *
 * Output format (france 485ef80): UFVK exports return a standard ZIP-316
 * `uview1...` string that Zashi / Zecwallet can import directly, preceded
 * by a one-line `# orchard-ufvk account=N birthday=H` metadata comment for
 * audit trail. OVK / IVK still ship as hex with the metadata header (no
 * standard text encoding exists for those sub-keys).
 */
export function ViewingKeyExportModal({ walletId, walletName, open, onClose }: ViewingKeyExportModalProps) {
  const { t } = useTranslation();

  const [step, setStep] = useState<Step>('form');
  const [keyType, setKeyType] = useState<ViewingKeyType>('ufvk');
  const [password, setPassword] = useState('');
  const [acknowledged, setAcknowledged] = useState(false);

  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [exportResult, setExportResult] = useState<ViewingKeyExportResponse | null>(null);
  const [keyText, setKeyText] = useState<string | null>(null);

  function reset() {
    setStep('form');
    setKeyType('ufvk');
    setPassword('');
    setAcknowledged(false);
    setError(null);
    setExportResult(null);
    setKeyText(null);
  }

  function close() {
    reset();
    onClose();
  }

  async function onSubmitExport() {
    setSubmitting(true);
    setError(null);
    try {
      const resp = await viewingKeyService.exportViewingKey(walletId, {
        password,
        key_type: keyType,
      });
      setExportResult(resp);
      // Clear password from state immediately to minimize residency
      setPassword('');
      setStep('token');
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  async function onClaimDownload() {
    if (!exportResult) return;
    setSubmitting(true);
    setError(null);
    try {
      const dl = await viewingKeyService.downloadKey(exportResult.download_token);
      setKeyText(dl.key_text);
      setStep('downloaded');
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    // TODO: replace alert with a toast (T2 polish)
    alert(t('common.copied'));
  }

  return (
    <Modal isOpen={open} onClose={close} title={t('auditor.viewing_key.export_title')}>
      <div className="space-y-4">
        {step === 'form' && (
          <>
            <div className="rounded bg-yellow-50 border border-yellow-200 p-3 text-sm flex gap-2">
              <AlertTriangle className="w-5 h-5 text-yellow-600 flex-shrink-0" />
              <div>
                <p className="font-semibold text-yellow-900">{t('auditor.viewing_key.warning_title')}</p>
                <p className="text-yellow-800 mt-1">{t('auditor.viewing_key.warning_body')}</p>
              </div>
            </div>

            <div>
              <p className="text-sm text-gray-600">{t('auditor.viewing_key.wallet')}</p>
              <p className="font-medium">{walletName} (#{walletId})</p>
            </div>

            <label className="block text-sm">
              <span className="text-gray-600">{t('auditor.viewing_key.key_type')}</span>
              <select
                className="mt-1 w-full border rounded p-2"
                value={keyType}
                onChange={e => setKeyType(e.target.value as ViewingKeyType)}
              >
                <option value="ufvk">{t('auditor.viewing_key.key_type.ufvk')}</option>
                <option value="ivk">{t('auditor.viewing_key.key_type.ivk')}</option>
                <option value="ovk">{t('auditor.viewing_key.key_type.ovk')}</option>
              </select>
              <p className="text-xs text-gray-500 mt-1">{t(`auditor.viewing_key.key_type.${keyType}.hint`)}</p>
            </label>

            <label className="block text-sm">
              <span className="text-gray-600">{t('auditor.viewing_key.password_reverify')}</span>
              <input
                type="password"
                autoComplete="current-password"
                className="mt-1 w-full border rounded p-2"
                value={password}
                onChange={e => setPassword(e.target.value)}
              />
              <p className="text-xs text-gray-500 mt-1">{t('auditor.viewing_key.password_hint')}</p>
            </label>

            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={acknowledged}
                onChange={e => setAcknowledged(e.target.checked)}
              />
              <span>{t('auditor.viewing_key.confirm_understand')}</span>
            </label>

            {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}

            <div className="flex gap-2">
              <button
                className="btn-primary flex-1"
                disabled={!acknowledged || !password || submitting}
                onClick={onSubmitExport}
              >
                <Key className="w-4 h-4 inline mr-1" />
                {submitting ? t('common.loading') : t('auditor.viewing_key.export_now')}
              </button>
              <button className="btn-ghost" onClick={close}>
                {t('common.cancel')}
              </button>
            </div>
          </>
        )}

        {step === 'token' && exportResult && (
          <>
            <div className="rounded bg-blue-50 border border-blue-200 p-3 text-sm">
              <p className="font-semibold">{t('auditor.viewing_key.export_created')}</p>
              <p className="text-xs text-gray-600 mt-1">
                {t('auditor.viewing_key.export_id')}: {exportResult.export_id} ·{' '}
                {t('auditor.viewing_key.expires_at')}: {new Date(exportResult.expires_at).toLocaleString()}
              </p>
            </div>

            <p className="text-sm">{t('auditor.viewing_key.token_explainer')}</p>

            <div>
              <p className="text-sm font-semibold mb-1">{t('auditor.viewing_key.download_token')}</p>
              <div className="font-mono text-xs break-all p-2 bg-gray-50 rounded border">
                {exportResult.download_token}
              </div>
              <button className="btn-ghost mt-1" onClick={() => copyToClipboard(exportResult.download_token)}>
                <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
              </button>
            </div>

            {error && <div className="rounded bg-red-50 text-red-800 px-3 py-2 text-sm">{error}</div>}

            <div className="flex gap-2">
              <button className="btn-primary flex-1" disabled={submitting} onClick={onClaimDownload}>
                <Download className="w-4 h-4 inline mr-1" />
                {submitting ? t('common.loading') : t('auditor.viewing_key.claim_download_now')}
              </button>
              <button className="btn-ghost" onClick={close}>
                {t('auditor.viewing_key.give_token_to_auditor')}
              </button>
            </div>
          </>
        )}

        {step === 'downloaded' && keyText && (
          <>
            <div className="rounded bg-green-50 border border-green-200 p-3 text-sm">
              <p className="font-semibold text-green-900">{t('auditor.viewing_key.downloaded_title')}</p>
              <p className="text-green-800 mt-1">{t('auditor.viewing_key.downloaded_body')}</p>
            </div>

            <div>
              <p className="text-sm font-semibold mb-1">{t('auditor.viewing_key.key_string')}</p>
              <p className="text-xs text-gray-500 mb-1">{t('auditor.viewing_key.key_string_hint')}</p>
              <div className="font-mono text-xs break-all p-2 bg-gray-50 rounded border">{keyText}</div>
              <button className="btn-ghost mt-1" onClick={() => copyToClipboard(keyText)}>
                <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
              </button>
            </div>

            <button className="btn-primary w-full" onClick={close}>
              {t('common.done')}
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
