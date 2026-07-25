import React, { useState, useEffect, useRef } from 'react';
import {
  Plus,
  Upload,
  Trash2,
  Key,
  CheckCircle,
  RefreshCw,
  Copy,
  Check,
  Shield,
  Eye,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Card, LoadingSpinner, Modal } from '../../../components/Common';
import { walletService } from '../../../services/api';
import orchardApi from '../../../services/api/orchard';
import { Wallet, BalanceResponse } from '../../../types';
import type {
  UnifiedAddressInfo,
  ShieldedBalance,
  ShieldedBalanceByPool,
  StoredOrchardNote,
} from '../../../types/orchard';
import { zatoshisToZec } from '../../../types/orchard';
import { useAuth } from '../../../hooks/useAuth';
import { getChain, ChainConfig } from '../../../config/chains';

interface ChainWalletsProps {
  chainId: string;
  /** Advisory strip rendered under the page header (F4 migration banners). */
  banner?: React.ReactNode;
}

export function ChainWallets({ chainId, banner }: ChainWalletsProps) {
  const { t } = useTranslation();
  const { user } = useAuth();
  const chain = getChain(chainId) as ChainConfig;

  const [wallets, setWallets] = useState<Wallet[]>([]);
  const [balances, setBalances] = useState<Record<string, BalanceResponse>>({});
  const [shieldedBalances, setShieldedBalances] = useState<Record<number, ShieldedBalance>>({});
  // F4.0-c: the same shielded balance split per pool, so a wallet mid-migration
  // shows the legacy pool draining and Ironwood filling instead of one number.
  const [poolBalances, setPoolBalances] = useState<Record<number, ShieldedBalanceByPool>>({});
  const [unifiedAddresses, setUnifiedAddresses] = useState<Record<number, UnifiedAddressInfo>>({});
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  // Modal states
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);
  const [showExportModal, setShowExportModal] = useState(false);
  const [showPrivacyAddressModal, setShowPrivacyAddressModal] = useState(false);
  const [showNotesModal, setShowNotesModal] = useState(false);
  const [selectedWalletId, setSelectedWalletId] = useState<number | null>(null);
  const [selectedNotes, setSelectedNotes] = useState<StoredOrchardNote[]>([]);
  const [loadingNotes, setLoadingNotes] = useState(false);
  const [generatingPrivacyAddress, setGeneratingPrivacyAddress] = useState<number | null>(null);
  const [copiedAddress, setCopiedAddress] = useState<string | null>(null);

  // Form states
  const [walletName, setWalletName] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [password, setPassword] = useState('');
  const [exportedKey, setExportedKey] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);

  const isLoadingRef = useRef(false);

  useEffect(() => {
    if (isLoadingRef.current) return;
    isLoadingRef.current = true;
    loadWallets().finally(() => {
      isLoadingRef.current = false;
    });
  }, [chainId]);

  const loadWallets = async () => {
    setIsLoading(true);
    try {
      const data = await walletService.listWallets(chainId);
      setWallets(data);

      // Load balances for all wallets
      const balancePromises = data.map((w) =>
        walletService.getBalance(w.address, w.chain).catch(() => null)
      );
      const results = await Promise.all(balancePromises);
      const newBalances: Record<string, BalanceResponse> = {};
      results.forEach((balance, index) => {
        if (balance) {
          newBalances[data[index].address] = balance;
        }
      });
      setBalances(newBalances);

      // For Zcash wallets, load existing unified addresses and shielded balances
      if (chainId === 'zcash') {
        const addressPromises = data.map((w) =>
          orchardApi.getUnifiedAddresses(w.id).catch(() => null)
        );
        const addressResults = await Promise.all(addressPromises);
        const newUnifiedAddresses: Record<number, UnifiedAddressInfo> = {};
        addressResults.forEach((addresses, index) => {
          // Get the first unified address if exists
          if (addresses && addresses.length > 0) {
            newUnifiedAddresses[data[index].id] = addresses[0];
          }
        });
        setUnifiedAddresses(newUnifiedAddresses);

        // Load shielded balances for wallets with unified addresses
        const walletsWithOrchard = data.filter((w) => newUnifiedAddresses[w.id]);
        if (walletsWithOrchard.length > 0) {
          const shieldedPromises = walletsWithOrchard.map((w) =>
            orchardApi.getShieldedBalance(w.id).catch(() => null)
          );
          const shieldedResults = await Promise.all(shieldedPromises);
          const newShieldedBalances: Record<number, ShieldedBalance> = {};
          shieldedResults.forEach((balance, index) => {
            if (balance) {
              newShieldedBalances[walletsWithOrchard[index].id] = balance;
            }
          });
          setShieldedBalances(newShieldedBalances);

          // Per-pool split. Advisory: a failure here leaves the headline
          // shielded balance above intact.
          const poolResults = await Promise.all(
            walletsWithOrchard.map((w) =>
              orchardApi.getShieldedBalanceByPool(w.id).catch(() => null)
            )
          );
          const newPoolBalances: Record<number, ShieldedBalanceByPool> = {};
          poolResults.forEach((split, index) => {
            if (split) {
              newPoolBalances[walletsWithOrchard[index].id] = split;
            }
          });
          setPoolBalances(newPoolBalances);
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load wallets');
    } finally {
      setIsLoading(false);
    }
  };

  const handleCreate = async () => {
    if (!walletName.trim()) return;
    setIsSubmitting(true);
    try {
      await walletService.createWallet({ name: walletName, chain: chainId });
      setSuccess(t('wallets.createSuccess'));
      setShowCreateModal(false);
      setWalletName('');
      loadWallets();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create wallet');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleImport = async () => {
    if (!walletName.trim() || !privateKey.trim()) return;
    setIsSubmitting(true);
    try {
      await walletService.importWallet({
        name: walletName,
        private_key: privateKey,
        chain: chainId,
      });
      setSuccess(t('wallets.importSuccess'));
      setShowImportModal(false);
      setWalletName('');
      setPrivateKey('');
      loadWallets();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to import wallet');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleExportKey = async () => {
    if (!selectedWalletId || !password.trim()) return;
    setIsSubmitting(true);
    try {
      const result = await walletService.exportPrivateKey(selectedWalletId, password);
      setExportedKey(result.private_key);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export key');
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleSetActive = async (id: number) => {
    try {
      await walletService.setActiveWallet(id);
      setSuccess(t('wallets.activeSuccess'));
      loadWallets();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to set active wallet');
    }
  };

  const handleDelete = async (id: number) => {
    if (!confirm(t('wallets.confirmDelete'))) return;
    try {
      await walletService.deleteWallet(id);
      setSuccess(t('wallets.deleteSuccess'));
      loadWallets();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete wallet');
    }
  };

  const copyToClipboard = async (text: string) => {
    if (!text) {
      setError(t('wallets.copyFailed', 'No address to copy'));
      return;
    }

    try {
      // Try modern clipboard API first
      if (navigator.clipboard && window.isSecureContext) {
        await navigator.clipboard.writeText(text);
      } else {
        // Fallback for older browsers or non-secure contexts
        const textArea = document.createElement('textarea');
        textArea.value = text;
        textArea.style.position = 'fixed';
        textArea.style.left = '-999999px';
        textArea.style.top = '-999999px';
        document.body.appendChild(textArea);
        textArea.focus();
        textArea.select();
        const successful = document.execCommand('copy');
        document.body.removeChild(textArea);
        if (!successful) {
          throw new Error('execCommand copy failed');
        }
      }
      setCopiedAddress(text);
      setSuccess(t('wallets.copiedToClipboard'));
      setTimeout(() => setCopiedAddress(null), 2000);
    } catch (err) {
      console.error('Copy to clipboard failed:', err);
      setError(t('wallets.copyFailed', 'Failed to copy to clipboard'));
    }
  };

  // View privacy notes for a wallet
  const handleViewNotes = async (walletId: number) => {
    setLoadingNotes(true);
    setSelectedWalletId(walletId);
    setShowNotesModal(true);
    try {
      const notes = await orchardApi.getUnspentNotes(walletId);
      setSelectedNotes(notes);
    } catch (err) {
      console.error('Error loading notes:', err);
      setSelectedNotes([]);
    } finally {
      setLoadingNotes(false);
    }
  };

  // Generate privacy address for Zcash wallet
  const handleGeneratePrivacyAddress = async (walletId: number) => {
    setGeneratingPrivacyAddress(walletId);
    setError('');
    try {
      const response = await orchardApi.enableOrchard({
        wallet_id: walletId,
        birthday_height: 2000000, // TODO: Get current block height
      });
      console.log('Orchard enable response:', response);
      if (response && response.unified_address && response.unified_address.address) {
        setUnifiedAddresses((prev) => ({
          ...prev,
          [walletId]: response.unified_address,
        }));
        setSuccess(t('zcash.orchard.enableSuccess', 'Privacy address generated successfully!'));
      } else {
        console.error('Invalid response structure:', response);
        setError('Unexpected response format from server');
      }
    } catch (err: any) {
      console.error('Error generating privacy address:', err);
      setError(err.response?.data?.message || err.message || 'Failed to generate privacy address');
    } finally {
      setGeneratingPrivacyAddress(null);
    }
  };

  const isAdmin = user?.role === 'admin';

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <LoadingSpinner size="lg" />
      </div>
    );
  }

  return (
    <div>
      <div className="mb-5 flex items-start justify-between gap-4">
        <div className="flex items-center gap-3">
          <span
            className="flex h-9 w-9 items-center justify-center rounded-full text-lg text-white"
            style={{ backgroundColor: chain.color }}
          >
            {chain.icon}
          </span>
          <div>
            <h1 className="text-[1.375rem] font-semibold leading-tight tracking-[-0.01em] text-ink-900">
              {chain.name} {t('wallets.title')}
            </h1>
            <p className="mt-1 text-[0.8125rem] text-ink-400">
              {t('chains.manageWallets', { chain: chain.name })}
            </p>
          </div>
        </div>
        {isAdmin && (
          <div className="flex shrink-0 gap-2">
            <button onClick={() => setShowCreateModal(true)} className="btn-primary">
              <Plus className="h-4 w-4" />
              {t('wallets.createWallet')}
            </button>
            <button onClick={() => setShowImportModal(true)} className="btn-secondary">
              <Upload className="h-4 w-4" />
              {t('wallets.importWallet')}
            </button>
          </div>
        )}
      </div>

      {banner && <div className="mb-5 space-y-2">{banner}</div>}

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded-lg text-red-700">
          {error}
          <button onClick={() => setError('')} className="ml-2 underline">
            {t('common.dismiss')}
          </button>
        </div>
      )}

      {success && (
        <div className="mb-4 p-3 bg-green-50 border border-green-200 rounded-lg text-green-700">
          {success}
          <button onClick={() => setSuccess('')} className="ml-2 underline">
            {t('common.dismiss')}
          </button>
        </div>
      )}

      <div className="grid gap-4">
        {wallets.map((wallet) => (
          <Card key={wallet.id}>
            <div className="flex items-start justify-between">
              <div className="flex-1">
                <div className="flex items-center">
                  <h3 className="text-lg font-semibold text-gray-900">
                    {wallet.name}
                  </h3>
                  {wallet.is_active && (
                    <span className="ml-2 px-2 py-0.5 bg-green-100 text-green-800 text-xs rounded-full">
                      {t('common.active')}
                    </span>
                  )}
                </div>

                {/* Transparent Address */}
                <div className="mt-2 flex items-center">
                  <code className="text-sm text-gray-600 bg-gray-100 px-2 py-1 rounded">
                    {wallet.address}
                  </code>
                  <button
                    onClick={() => copyToClipboard(wallet.address)}
                    className={`ml-2 transition-colors ${copiedAddress === wallet.address ? 'text-green-500' : 'text-gray-400 hover:text-gray-600'}`}
                    title={copiedAddress === wallet.address ? t('wallets.copied', 'Copied!') : t('common.copy')}
                  >
                    {copiedAddress === wallet.address ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                  </button>
                </div>

                {/* Zcash Privacy Address Section */}
                {chainId === 'zcash' && (
                  <div className="mt-3">
                    {unifiedAddresses[wallet.id]?.address ? (
                      // Show unified address if already generated
                      <div className="p-3 bg-green-50 border border-green-200 rounded-lg">
                        <div className="flex items-center gap-2 mb-2">
                          <Shield className="w-4 h-4 text-green-600" />
                          <span className="text-sm font-medium text-green-800">
                            {t('zcash.orchard.privacyAddress', 'Privacy Address (Unified)')}
                          </span>
                        </div>
                        <div className="flex items-center">
                          <code className="text-xs text-green-700 bg-green-100 px-2 py-1 rounded break-all">
                            {unifiedAddresses[wallet.id].address.slice(0, 30)}...
                          </code>
                          <button
                            onClick={() => copyToClipboard(unifiedAddresses[wallet.id]?.address || '')}
                            className={`ml-2 transition-colors ${copiedAddress === unifiedAddresses[wallet.id]?.address ? 'text-green-800' : 'text-green-600 hover:text-green-800'}`}
                            title={copiedAddress === unifiedAddresses[wallet.id]?.address ? t('wallets.copied', 'Copied!') : t('common.copy')}
                          >
                            {copiedAddress === unifiedAddresses[wallet.id]?.address ? <Check className="w-4 h-4" /> : <Copy className="w-4 h-4" />}
                          </button>
                        </div>
                      </div>
                    ) : (
                      // Show generate button if not generated
                      <button
                        onClick={() => handleGeneratePrivacyAddress(wallet.id)}
                        disabled={generatingPrivacyAddress === wallet.id}
                        className="flex items-center gap-2 px-3 py-2 text-sm bg-yellow-100 text-yellow-800 rounded-lg hover:bg-yellow-200 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                      >
                        {generatingPrivacyAddress === wallet.id ? (
                          <>
                            <LoadingSpinner size="sm" />
                            {t('zcash.orchard.generating', 'Generating...')}
                          </>
                        ) : (
                          <>
                            <Shield className="w-4 h-4" />
                            {t('zcash.orchard.generatePrivacyAddress', 'Generate Privacy Address')}
                          </>
                        )}
                      </button>
                    )}
                  </div>
                )}

                {balances[wallet.address] && (
                  <div className="mt-3">
                    <p className="text-sm text-gray-500">{t('wallets.balance')}</p>
                    {/* For Zcash: show total balance (transparent + shielded) */}
                    {chainId === 'zcash' && shieldedBalances[wallet.id] ? (
                      <div>
                        <p className="text-lg font-semibold">
                          {(parseFloat(balances[wallet.address].native_balance) + zatoshisToZec(shieldedBalances[wallet.id].total_zatoshis)).toFixed(6)} {chain.symbol}
                        </p>
                        <div className="mt-1 text-sm text-gray-500 space-y-1">
                          <div className="flex items-center justify-between">
                            <span>{t('zcash.transparentBalance', 'Transparent')}:</span>
                            <span>{parseFloat(balances[wallet.address].native_balance).toFixed(6)} {chain.symbol}</span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span className="flex items-center gap-1">
                              <Shield className="w-3 h-3 text-green-600" />
                              {t('zcash.shieldedBalance', 'Shielded')}:
                            </span>
                            <span className="flex items-center gap-2">
                              {zatoshisToZec(shieldedBalances[wallet.id].total_zatoshis).toFixed(6)} {chain.symbol}
                              {shieldedBalances[wallet.id].note_count > 0 && (
                                <button
                                  onClick={() => handleViewNotes(wallet.id)}
                                  className="text-xs text-blue-600 hover:text-blue-800 flex items-center gap-1"
                                  title={t('zcash.viewNotes', 'View notes')}
                                >
                                  <Eye className="w-3 h-3" />
                                  ({shieldedBalances[wallet.id].note_count})
                                </button>
                              )}
                            </span>
                          </div>
                          <PoolSplit split={poolBalances[wallet.id]} symbol={chain.symbol} />
                        </div>
                      </div>
                    ) : (
                      <p className="text-lg font-semibold">
                        {parseFloat(balances[wallet.address].native_balance).toFixed(6)} {chain.symbol}
                      </p>
                    )}
                    {balances[wallet.address].tokens.length > 0 && (
                      <div className="mt-1 flex flex-wrap gap-2">
                        {balances[wallet.address].tokens.map((token) => (
                          <span
                            key={token.symbol}
                            className="text-sm text-gray-600 bg-gray-100 px-2 py-0.5 rounded"
                          >
                            {parseFloat(token.balance).toFixed(2)} {token.symbol}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                )}
              </div>

              {isAdmin && (
                <div className="flex space-x-2">
                  <button
                    onClick={() => loadWallets()}
                    className="p-2 text-gray-400 hover:text-blue-600"
                    title={t('wallets.refreshBalance')}
                  >
                    <RefreshCw className="w-4 h-4" />
                  </button>
                  {!wallet.is_active && (
                    <button
                      onClick={() => handleSetActive(wallet.id)}
                      className="p-2 text-gray-400 hover:text-green-600"
                      title={t('wallets.setAsActive')}
                    >
                      <CheckCircle className="w-4 h-4" />
                    </button>
                  )}
                  <button
                    onClick={() => {
                      setSelectedWalletId(wallet.id);
                      setExportedKey('');
                      setPassword('');
                      setShowExportModal(true);
                    }}
                    className="p-2 text-gray-400 hover:text-yellow-600"
                    title={t('wallets.exportPrivateKey')}
                  >
                    <Key className="w-4 h-4" />
                  </button>
                  <button
                    onClick={() => handleDelete(wallet.id)}
                    className="p-2 text-gray-400 hover:text-red-600"
                    title={t('wallets.deleteWallet')}
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              )}
            </div>
          </Card>
        ))}

        {wallets.length === 0 && (
          <Card>
            <p className="text-center text-gray-500 py-8">
              {t('wallets.noWallets')}
            </p>
          </Card>
        )}
      </div>

      {/* Create Wallet Modal */}
      <Modal
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        title={`${t('wallets.createWallet')} - ${chain.name}`}
      >
        <div className="space-y-4">
          <div className="flex items-center p-3 bg-gray-50 rounded-lg">
            <span
              className="w-8 h-8 rounded-full flex items-center justify-center text-white mr-3"
              style={{ backgroundColor: chain.color }}
            >
              {chain.icon}
            </span>
            <div>
              <p className="font-medium">{chain.name}</p>
              <p className="text-sm text-gray-500">{t('chains.addressFormat')}: {chain.addressPrefix}...</p>
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t('wallets.walletName')}
            </label>
            <input
              type="text"
              value={walletName}
              onChange={(e) => setWalletName(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
              placeholder={t('wallets.walletNamePlaceholder')}
            />
          </div>
          <div className="flex justify-end space-x-3">
            <button
              onClick={() => setShowCreateModal(false)}
              className="px-4 py-2 text-gray-600 hover:text-gray-800"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={handleCreate}
              disabled={isSubmitting || !walletName.trim()}
              className="px-4 py-2 text-white rounded-lg hover:opacity-90 disabled:opacity-50"
              style={{ backgroundColor: chain.color }}
            >
              {isSubmitting ? <LoadingSpinner size="sm" /> : t('wallets.create')}
            </button>
          </div>
        </div>
      </Modal>

      {/* Import Wallet Modal */}
      <Modal
        isOpen={showImportModal}
        onClose={() => setShowImportModal(false)}
        title={`${t('wallets.importWallet')} - ${chain.name}`}
      >
        <div className="space-y-4">
          <div className="flex items-center p-3 bg-gray-50 rounded-lg">
            <span
              className="w-8 h-8 rounded-full flex items-center justify-center text-white mr-3"
              style={{ backgroundColor: chain.color }}
            >
              {chain.icon}
            </span>
            <div>
              <p className="font-medium">{chain.name}</p>
              <p className="text-sm text-gray-500">{t('chains.addressFormat')}: {chain.addressPrefix}...</p>
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t('wallets.walletName')}
            </label>
            <input
              type="text"
              value={walletName}
              onChange={(e) => setWalletName(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
              placeholder={t('wallets.walletNamePlaceholder')}
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              {t('wallets.privateKey')}
            </label>
            <input
              type="password"
              value={privateKey}
              onChange={(e) => setPrivateKey(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
              placeholder={chainId === 'ethereum' ? '0x...' : t('wallets.privateKeyPlaceholder')}
            />
          </div>
          <div className="flex justify-end space-x-3">
            <button
              onClick={() => setShowImportModal(false)}
              className="px-4 py-2 text-gray-600 hover:text-gray-800"
            >
              {t('common.cancel')}
            </button>
            <button
              onClick={handleImport}
              disabled={isSubmitting || !walletName.trim() || !privateKey.trim()}
              className="px-4 py-2 text-white rounded-lg hover:opacity-90 disabled:opacity-50"
              style={{ backgroundColor: chain.color }}
            >
              {isSubmitting ? <LoadingSpinner size="sm" /> : t('wallets.import')}
            </button>
          </div>
        </div>
      </Modal>

      {/* Export Private Key Modal */}
      <Modal
        isOpen={showExportModal}
        onClose={() => {
          setShowExportModal(false);
          setExportedKey('');
          setPassword('');
        }}
        title={t('wallets.exportKeyTitle')}
      >
        <div className="space-y-4">
          {!exportedKey ? (
            <>
              <div className="p-3 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-800 text-sm">
                {t('wallets.exportKeyWarning')}
              </div>
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {t('wallets.exportKeyConfirm')}
                </label>
                <input
                  type="password"
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                  placeholder={t('login.password')}
                />
              </div>
              <div className="flex justify-end space-x-3">
                <button
                  onClick={() => setShowExportModal(false)}
                  className="px-4 py-2 text-gray-600 hover:text-gray-800"
                >
                  {t('common.cancel')}
                </button>
                <button
                  onClick={handleExportKey}
                  disabled={isSubmitting || !password.trim()}
                  className="px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 disabled:opacity-50"
                >
                  {isSubmitting ? <LoadingSpinner size="sm" /> : t('wallets.export')}
                </button>
              </div>
            </>
          ) : (
            <>
              <div className="p-3 bg-red-50 border border-red-200 rounded-lg text-red-800 text-sm">
                {t('wallets.keepSecure')}
              </div>
              <div className="p-3 bg-gray-100 rounded-lg">
                <code className="text-sm break-all">{exportedKey}</code>
              </div>
              <div className="flex justify-end space-x-3">
                <button
                  onClick={() => copyToClipboard(exportedKey)}
                  className={`px-4 py-2 text-white rounded-lg transition-colors ${copiedAddress === exportedKey ? 'bg-green-600 hover:bg-green-700' : 'bg-gray-600 hover:bg-gray-700'}`}
                >
                  {copiedAddress === exportedKey ? t('wallets.copied', 'Copied!') : t('common.copy')}
                </button>
                <button
                  onClick={() => {
                    setShowExportModal(false);
                    setExportedKey('');
                    setPassword('');
                  }}
                  className="px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700"
                >
                  {t('common.done')}
                </button>
              </div>
            </>
          )}
        </div>
      </Modal>

      {/* Privacy Notes Modal */}
      <Modal
        isOpen={showNotesModal}
        onClose={() => {
          setShowNotesModal(false);
          setSelectedNotes([]);
        }}
        title={t('zcash.privacyNotes', 'Privacy Notes')}
      >
        <div className="space-y-4">
          {loadingNotes ? (
            <div className="flex items-center justify-center py-8">
              <LoadingSpinner size="lg" />
            </div>
          ) : selectedNotes.length === 0 ? (
            <p className="text-center text-gray-500 py-8">
              {t('zcash.noNotes', 'No privacy notes found')}
            </p>
          ) : (
            <>
              <div className="text-sm text-gray-600 mb-2">
                {t('zcash.orchard.notesCount', { count: selectedNotes.length })}
              </div>
              <div className="max-h-96 overflow-y-auto space-y-3">
                {selectedNotes.map((note) => (
                  <div
                    key={note.id}
                    className="p-3 bg-gray-50 border border-gray-200 rounded-lg"
                  >
                    <div className="flex items-center justify-between mb-2">
                      <span className="font-semibold text-green-700">
                        {note.value_zec.toFixed(8)} ZEC
                      </span>
                      <span className="text-xs text-gray-500">
                        {t('zcash.blockHeight', 'Block')}: {note.block_height.toLocaleString()}
                      </span>
                    </div>
                    <div className="text-xs text-gray-500 space-y-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{t('zcash.txHash', 'TX')}:</span>
                        <code className="bg-gray-100 px-1 rounded">
                          {note.tx_hash.slice(0, 16)}...{note.tx_hash.slice(-8)}
                        </code>
                        <button
                          onClick={() => copyToClipboard(note.tx_hash)}
                          className="text-gray-400 hover:text-gray-600"
                        >
                          {copiedAddress === note.tx_hash ? (
                            <Check className="w-3 h-3 text-green-500" />
                          ) : (
                            <Copy className="w-3 h-3" />
                          )}
                        </button>
                      </div>
                      {note.memo && (
                        <div>
                          <span className="font-medium">{t('zcash.memo', 'Memo')}:</span>{' '}
                          {note.memo}
                        </div>
                      )}
                    </div>
                  </div>
                ))}
              </div>
              <div className="pt-2 border-t border-gray-200">
                <div className="flex items-center justify-between text-sm font-semibold">
                  <span>{t('zcash.totalShielded', 'Total Shielded')}:</span>
                  <span className="text-green-700">
                    {selectedNotes.reduce((sum, n) => sum + n.value_zec, 0).toFixed(8)} ZEC
                  </span>
                </div>
              </div>
            </>
          )}
          <div className="flex justify-end">
            <button
              onClick={() => {
                setShowNotesModal(false);
                setSelectedNotes([]);
              }}
              className="px-4 py-2 bg-gray-600 text-white rounded-lg hover:bg-gray-700"
            >
              {t('common.close')}
            </button>
          </div>
        </div>
      </Modal>
    </div>
  );
}

/**
 * F4.0-c — legacy Orchard vs Ironwood split under the shielded balance.
 *
 * Rendered only once Ironwood holds something: before a wallet migrates, every
 * shielded zatoshi is in the old pool and a second line would be noise. During
 * and after a migration it answers the question the headline number hides —
 * how much is still stuck behind the turnstile.
 */
function PoolSplit({
  split,
  symbol,
}: {
  split?: ShieldedBalanceByPool;
  symbol: string;
}) {
  const { t } = useTranslation();
  if (!split) return null;

  const amount = (pool: string) =>
    split.pools.find((p) => p.pool === pool)?.total_zatoshis ?? 0;
  const legacy = amount('orchard');
  const ironwood = amount('ironwood');
  if (ironwood === 0) return null;

  return (
    <div className="ml-1 space-y-1 border-l border-line-200 pl-3 text-[0.75rem]">
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-legacy-700">
          <span className="h-1.5 w-1.5 rounded-full bg-legacy-500" />
          {t('migration.pool.legacy')}
        </span>
        <span className="num">
          {zatoshisToZec(legacy).toFixed(6)} {symbol}
        </span>
      </div>
      <div className="flex items-center justify-between">
        <span className="flex items-center gap-1.5 text-iron-700">
          <span className="h-1.5 w-1.5 rounded-full bg-iron-500" />
          {t('migration.pool.ironwood')}
        </span>
        <span className="num">
          {zatoshisToZec(ironwood).toFixed(6)} {symbol}
        </span>
      </div>
    </div>
  );
}
