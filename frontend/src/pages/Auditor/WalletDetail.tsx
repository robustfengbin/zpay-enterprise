import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useNavigate } from 'react-router-dom';
import { ArrowLeft, RefreshCw, FileText } from 'lucide-react';
import { Card, LoadingSpinner } from '../../components/Common';
import { viewingKeyService } from '../../services/api';
import type {
  AuditorTenantSummary,
  AuditorTransfersResponse,
  AuditorWalletBalance,
} from '../../types/viewing-key';

/**
 * F1.1 §2 — Auditor wallet detail page.
 *
 * Drill-down from the dashboard: live balance (transparent + shielded for
 * Zcash) + transfers paged within the scope window. Raw shielded amounts
 * still need a disclosure to decrypt; this view gives the auditor the
 * envelope (sender, recipient, status, time) that the scope window allows.
 */
export function AuditorWalletDetail() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { id } = useParams<{ id: string }>();
  const walletId = Number(id);

  const [wallet, setWallet] = useState<AuditorTenantSummary | null>(null);
  const [balance, setBalance] = useState<AuditorWalletBalance | null>(null);
  const [transfers, setTransfers] = useState<AuditorTransfersResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const PAGE = 25;

  async function load() {
    if (!Number.isFinite(walletId)) {
      setError(t('common.not_found'));
      setLoading(false);
      return;
    }
    setLoading(true);
    try {
      const [walletList, bal, tx] = await Promise.all([
        viewingKeyService.listAuditorWallets(),
        viewingKeyService.getAuditorWalletBalance(walletId),
        viewingKeyService.getAuditorWalletTransfers(walletId, PAGE, offset),
      ]);
      const w = walletList.find(x => x.wallet_id === walletId) || null;
      setWallet(w);
      setBalance(bal);
      setTransfers(tx);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => { void load(); }, [walletId, offset]);

  if (loading) return <LoadingSpinner />;

  return (
    <div className="p-6 max-w-6xl space-y-4">
      <header className="flex items-center justify-between">
        <div>
          <button className="btn-ghost mb-2" onClick={() => navigate('/auditor')}>
            <ArrowLeft className="w-4 h-4 inline mr-1" /> {t('auditor.detail.back')}
          </button>
          <h1 className="text-2xl font-semibold">
            {wallet?.wallet_name || `${t('auditor.detail.wallet')} #${walletId}`}
          </h1>
          {wallet && (
            <p className="text-xs text-gray-500 font-mono">{wallet.address}</p>
          )}
        </div>
        <div className="flex gap-2">
          <button className="btn-ghost" onClick={() => void load()} aria-label={t('common.refresh')}>
            <RefreshCw className="w-4 h-4" />
          </button>
          {wallet && (
            <>
              <button
                className="btn-ghost"
                onClick={() => navigate(`/auditor/wallets/${wallet.wallet_id}/disclosures`)}
              >
                <FileText className="w-4 h-4 inline mr-1" />
                {t('auditor.disclosure.history_title')}
              </button>
              <button
                className="btn-secondary"
                disabled={wallet.current_count >= wallet.max_disclosure_count}
                onClick={() => navigate(`/auditor/disclosure/new?wallet_id=${wallet.wallet_id}`)}
              >
                <FileText className="w-4 h-4 inline mr-1" />
                {t('auditor.dashboard.request_disclosure')}
              </button>
            </>
          )}
        </div>
      </header>

      {error && <div className="rounded bg-red-50 text-red-800 px-4 py-2 text-sm">{error}</div>}

      {wallet && (
        <Card>
          <h2 className="text-sm font-semibold mb-2">{t('auditor.detail.scope_section')}</h2>
          <dl className="grid grid-cols-4 gap-y-2 text-sm">
            <dt className="text-gray-500">{t('auditor.detail.scope_start')}</dt>
            <dd>{new Date(wallet.scope_start).toLocaleString()}</dd>
            <dt className="text-gray-500">{t('auditor.detail.scope_end')}</dt>
            <dd>{new Date(wallet.scope_end).toLocaleString()}</dd>
            <dt className="text-gray-500">{t('auditor.dashboard.col.budget')}</dt>
            <dd>{wallet.current_count} / {wallet.max_disclosure_count}</dd>
            <dt className="text-gray-500">{t('auditor.dashboard.col.tx_count')}</dt>
            <dd>{wallet.total_tx_count}</dd>
          </dl>
        </Card>
      )}

      {balance && (
        <Card>
          <h2 className="text-sm font-semibold mb-2">{t('auditor.detail.balance_section')}</h2>
          <dl className="grid grid-cols-4 gap-y-2 text-sm">
            <dt className="text-gray-500">{t('auditor.detail.native_balance')}</dt>
            <dd className="font-mono">{balance.native_balance}</dd>
            <dt className="text-gray-500">{t('auditor.detail.chain')}</dt>
            <dd>{balance.chain}</dd>
          </dl>
          {balance.tokens.length > 0 && (
            <table className="w-full text-xs mt-3">
              <thead className="text-gray-500 uppercase">
                <tr>
                  <th className="text-left p-2">{t('auditor.detail.col.token_symbol')}</th>
                  <th className="text-left p-2">{t('auditor.detail.col.token_contract')}</th>
                  <th className="text-right p-2">{t('auditor.detail.col.token_balance')}</th>
                </tr>
              </thead>
              <tbody>
                {balance.tokens.map((tk, i) => (
                  <tr key={i} className="border-t border-gray-100">
                    <td className="p-2 font-medium">{tk.symbol}</td>
                    <td className="p-2 font-mono text-xs truncate max-w-xs">{tk.contract}</td>
                    <td className="p-2 text-right font-mono">{tk.balance}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Card>
      )}

      <Card>
        <h2 className="text-sm font-semibold mb-2">{t('auditor.detail.transfers_section')}</h2>
        {transfers && (
          <p className="text-xs text-gray-500 mb-2">
            {t('auditor.detail.transfers_window', {
              start: new Date(transfers.scope_start).toLocaleDateString(),
              end: new Date(transfers.scope_end).toLocaleDateString(),
            })}
          </p>
        )}
        <table className="w-full text-xs">
          <thead className="text-gray-500 uppercase">
            <tr>
              <th className="text-left p-2">#</th>
              <th className="text-left p-2">{t('auditor.detail.col.time')}</th>
              <th className="text-left p-2">{t('auditor.detail.col.direction')}</th>
              <th className="text-left p-2">{t('auditor.detail.col.counterparty')}</th>
              <th className="text-right p-2">{t('auditor.detail.col.amount')}</th>
              <th className="text-left p-2">{t('auditor.detail.col.token')}</th>
              <th className="text-left p-2">{t('auditor.detail.col.status')}</th>
              <th className="text-left p-2">{t('auditor.detail.col.tx')}</th>
            </tr>
          </thead>
          <tbody>
            {transfers?.transfers.map(tx => (
              <tr key={tx.id} className="border-t border-gray-100">
                <td className="p-2 font-mono">{tx.id}</td>
                <td className="p-2 text-xs">{new Date(tx.created_at).toLocaleString()}</td>
                <td className="p-2">
                  {wallet && tx.from_address.toLowerCase() === wallet.address.toLowerCase()
                    ? t('auditor.detail.dir_out')
                    : t('auditor.detail.dir_in')}
                </td>
                <td className="p-2 font-mono text-xs truncate max-w-[200px]">
                  {wallet && tx.from_address.toLowerCase() === wallet.address.toLowerCase()
                    ? tx.to_address
                    : tx.from_address}
                </td>
                <td className="p-2 text-right font-mono">{tx.amount}</td>
                <td className="p-2">{tx.token}</td>
                <td className="p-2">
                  <span className="text-xs px-1.5 py-0.5 rounded bg-gray-100 text-gray-800">{tx.status}</span>
                </td>
                <td className="p-2 font-mono text-xs truncate max-w-[140px]" title={tx.tx_hash || ''}>
                  {tx.tx_hash || '—'}
                </td>
              </tr>
            ))}
            {transfers?.transfers.length === 0 && (
              <tr><td colSpan={8} className="p-4 text-center text-gray-400">{t('auditor.detail.empty_transfers')}</td></tr>
            )}
          </tbody>
        </table>
        <div className="mt-3 flex gap-2 items-center text-xs">
          <button
            className="btn-ghost"
            disabled={offset === 0}
            onClick={() => setOffset(Math.max(0, offset - PAGE))}
          >
            ← {t('common.previous')}
          </button>
          <button
            className="btn-ghost"
            disabled={!transfers || transfers.transfers.length < PAGE}
            onClick={() => setOffset(offset + PAGE)}
          >
            {t('common.next')} →
          </button>
          <span className="text-gray-500">
            {t('auditor.detail.page_label', { from: offset + 1, to: offset + (transfers?.transfers.length || 0) })}
          </span>
        </div>
      </Card>
    </div>
  );
}
