import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Copy, Download, Key, AlertTriangle } from 'lucide-react';
import { Card, Modal } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type { ViewingKeyExportResponse } from '../../types/viewing-key';

interface ViewingKeyExportModalProps {
  walletId: number;
  walletName: string;
  open: boolean;
  onClose: () => void;
}

/**
 * F1.1 §2 — Admin one-click export of Orchard UFVK for an auditor.
 * Exports the standard ZIP-316 UFVK (Zashi-compatible) per france's
 * reconnaissance note. Legacy "ufvk:account:birthday:hex" optionally
 * included for internal round-trip.
 *
 * WARNING: viewing key gives read access to all incoming + outgoing
 * shielded notes for this wallet. UI uses a strong yellow warning banner
 * and double-confirm pattern.
 */
export function ViewingKeyExportModal({ walletId, walletName, open, onClose }: ViewingKeyExportModalProps) {
  const { t } = useTranslation();
  const [includeLegacy, setIncludeLegacy] = useState(false);
  const [result, setResult] = useState<ViewingKeyExportResponse | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [acknowledged, setAcknowledged] = useState(false);

  function reset() {
    setResult(null);
    setAcknowledged(false);
    setIncludeLegacy(false);
    setError(null);
  }

  async function onExport() {
    setSubmitting(true);
    setError(null);
    try {
      const data = await viewingKeyService.exportViewingKey({ wallet_id: walletId, include_legacy: includeLegacy });
      setResult(data);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSubmitting(false);
    }
  }

  function copyToClipboard(text: string) {
    navigator.clipboard.writeText(text);
    // Toast not yet wired; alert is a temp stand-in
    alert(t('common.copied'));
  }

  return (
    <Modal isOpen={open} onClose={() => { reset(); onClose(); }} title={t('auditor.viewing_key.export_title')}>
      <div className="space-y-4">
        {!result ? (
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

            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={includeLegacy}
                onChange={e => setIncludeLegacy(e.target.checked)}
              />
              <span>{t('auditor.viewing_key.include_legacy')}</span>
              <span className="text-xs text-gray-500">{t('auditor.viewing_key.include_legacy_hint')}</span>
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
                disabled={!acknowledged || submitting}
                onClick={onExport}
              >
                <Key className="w-4 h-4 inline mr-1" />
                {t('auditor.viewing_key.export_now')}
              </button>
              <button className="btn-ghost" onClick={() => { reset(); onClose(); }}>
                {t('common.cancel')}
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="rounded bg-green-50 border border-green-200 p-3 text-sm">
              {t('auditor.viewing_key.exported_at')}: {new Date(result.exported_at).toLocaleString()}
            </div>

            <div>
              <p className="text-sm font-semibold mb-1">{t('auditor.viewing_key.standard_ufvk')}</p>
              <p className="text-xs text-gray-500 mb-1">{t('auditor.viewing_key.standard_hint')}</p>
              <div className="font-mono text-xs break-all p-2 bg-gray-50 rounded border">{result.ufvk_standard}</div>
              <button className="btn-ghost mt-1" onClick={() => copyToClipboard(result.ufvk_standard)}>
                <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
              </button>
            </div>

            {result.ufvk_legacy && (
              <div>
                <p className="text-sm font-semibold mb-1">{t('auditor.viewing_key.legacy_ufvk')}</p>
                <p className="text-xs text-gray-500 mb-1">{t('auditor.viewing_key.legacy_hint')}</p>
                <div className="font-mono text-xs break-all p-2 bg-gray-50 rounded border">{result.ufvk_legacy}</div>
                <button className="btn-ghost mt-1" onClick={() => copyToClipboard(result.ufvk_legacy!)}>
                  <Copy className="w-3 h-3 inline mr-1" />{t('common.copy')}
                </button>
              </div>
            )}

            <p className="text-xs text-gray-600 italic">{result.warning}</p>

            <button className="btn-primary w-full" onClick={() => { reset(); onClose(); }}>
              <Download className="w-4 h-4 inline mr-1" />
              {t('common.done')}
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
