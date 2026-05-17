import React, { useState, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { ArrowRight, AlertCircle, CheckCircle, Wallet, Fuel, Calculator, Clock } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Card, LoadingSpinner } from '../../../components/Common';
import { walletService, transferService } from '../../../services/api';
import { Wallet as WalletType, Transfer as TransferType, BalanceResponse } from '../../../types';
import type { TransferApprovalFields } from '../../../types/approval';
import { useAuth } from '../../../hooks/useAuth';
import { getChain, ChainConfig, isNativeToken } from '../../../config/chains';
import type { GasEstimateResponse } from '../../../services/api/transfer';

/**
 * Transfer DTO augmented with F2.1 approval fields. Backend returns these
 * on the initiate response when the amount triggers a matching
 * approval_policies row (see france 899cbeb auto-pivot).
 */
type TransferInitiateResponse = TransferType & Partial<TransferApprovalFields>;

interface ChainTransferProps {
  chainId: string;
}

export function ChainTransfer({ chainId }: ChainTransferProps) {
  const { t } = useTranslation();
  const { user } = useAuth();
  const navigate = useNavigate();
  const chain = getChain(chainId) as ChainConfig;

  const [wallets, setWallets] = useState<WalletType[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');
  const [pendingTransfer, setPendingTransfer] = useState<TransferType | null>(null);

  // Wallet balance state
  const [walletBalance, setWalletBalance] = useState<BalanceResponse | null>(null);
  const [isLoadingBalance, setIsLoadingBalance] = useState(false);

  // Gas estimate state
  const [gasEstimate, setGasEstimate] = useState<GasEstimateResponse | null>(null);
  const [isEstimating, setIsEstimating] = useState(false);

  // Form state
  const [toAddress, setToAddress] = useState('');
  const [token, setToken] = useState(chain.symbol);
  const [amount, setAmount] = useState('');
  const [gasPriceGwei, setGasPriceGwei] = useState('');
  const [gasLimit, setGasLimit] = useState('');

  useEffect(() => {
    loadData();
  }, [chainId]);

  // Reset token when chain changes
  useEffect(() => {
    setToken(chain.symbol);
  }, [chain.symbol]);

  const activeWallet = wallets.find((w) => w.is_active && w.chain === chainId);

  // Load wallet balance when active wallet changes
  useEffect(() => {
    if (activeWallet) {
      loadWalletBalance(activeWallet.address, activeWallet.chain);
    } else {
      setWalletBalance(null);
    }
  }, [activeWallet?.address, activeWallet?.chain]);

  const loadData = async () => {
    setIsLoading(true);
    try {
      const walletsData = await walletService.listWallets(chainId);
      setWallets(walletsData);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally {
      setIsLoading(false);
    }
  };

  const loadWalletBalance = async (address: string, walletChain: string) => {
    setIsLoadingBalance(true);
    try {
      const balance = await walletService.getBalance(address, walletChain);
      setWalletBalance(balance);
    } catch (err) {
      console.error('Failed to load balance:', err);
      setWalletBalance(null);
    } finally {
      setIsLoadingBalance(false);
    }
  };

  const handleInitiate = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSuccess('');
    setIsSubmitting(true);
    setGasEstimate(null);

    try {
      // First estimate gas/fee
      setIsEstimating(true);
      const estimate = await transferService.estimateGas({
        chain: chainId,
        to_address: toAddress,
        token,
        amount,
      });
      setGasEstimate(estimate);
      setIsEstimating(false);

      // Then initiate transfer
      const transfer = (await transferService.initiateTransfer({
        chain: chainId,
        to_address: toAddress,
        token,
        amount,
        gas_price_gwei: gasPriceGwei || undefined,
        gas_limit: gasLimit ? parseInt(gasLimit) : undefined,
      })) as TransferInitiateResponse;

      // F2.1 auto-pivot: if the amount triggered an approval policy on
      // the backend (france 899cbeb), the response carries
      // approval_required=true and status='awaiting_approval'. In that
      // case the user can NOT execute directly — redirect them to their
      // "my pending" list so they can track the checker's decision.
      if (transfer.approval_required || (transfer.status as string) === 'awaiting_approval') {
        setSuccess(t('transfer.awaiting_approval_redirect'));
        resetForm();
        setGasEstimate(null);
        setPendingTransfer(null);
        // brief pause so the user sees the success banner before navigation
        setTimeout(() => navigate('/approval/pending'), 1200);
      } else {
        setPendingTransfer(transfer);
        setSuccess(t('transfer.initiateSuccess'));
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to initiate transfer');
      setIsEstimating(false);
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleExecute = async () => {
    if (!pendingTransfer) return;
    setIsSubmitting(true);
    setError('');

    try {
      await transferService.executeTransfer(pendingTransfer.id);
      setSuccess(t('transfer.executeSuccess'));
      setPendingTransfer(null);
      setGasEstimate(null);
      resetForm();
      // Refresh balance after transfer
      if (activeWallet) {
        loadWalletBalance(activeWallet.address, activeWallet.chain);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to execute transfer');
    } finally {
      setIsSubmitting(false);
    }
  };

  const resetForm = () => {
    setToAddress('');
    setAmount('');
    setGasPriceGwei('');
    setGasLimit('');
    setPendingTransfer(null);
    setGasEstimate(null);
  };

  // Get current token balance
  const getCurrentTokenBalance = () => {
    if (!walletBalance) return '0';
    if (isNativeToken(chainId, token)) {
      return walletBalance.native_balance;
    }
    const tokenBalance = walletBalance.tokens.find(t => t.symbol === token);
    return tokenBalance?.balance || '0';
  };

  // Calculate after-transfer balance
  const getAfterTransferBalance = () => {
    const currentBalance = parseFloat(getCurrentTokenBalance());
    const transferAmount = parseFloat(amount) || 0;
    const afterBalance = currentBalance - transferAmount;
    return Math.max(0, afterBalance).toFixed(6);
  };

  // Calculate native token balance after fee
  const getNativeAfterFee = () => {
    if (!walletBalance || !gasEstimate) return null;
    const nativeBalance = parseFloat(walletBalance.native_balance);
    const fee = parseFloat(gasEstimate.estimated_fee_eth);
    const transferAmount = isNativeToken(chainId, token) ? parseFloat(amount) || 0 : 0;
    const afterBalance = nativeBalance - fee - transferAmount;
    return Math.max(0, afterBalance).toFixed(6);
  };

  const isAdmin = user?.role === 'admin';
  const isEVM = chainId === 'ethereum'; // For gas-related UI

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-64">
        <LoadingSpinner size="lg" />
      </div>
    );
  }

  if (!isAdmin) {
    return (
      <Card>
        <div className="text-center py-8">
          <AlertCircle className="w-12 h-12 text-yellow-500 mx-auto mb-4" />
          <p className="text-gray-600">{t('transfer.adminOnly')}</p>
        </div>
      </Card>
    );
  }

  return (
    <div>
      <div className="flex items-center mb-6">
        <span
          className="w-10 h-10 rounded-full flex items-center justify-center text-white text-xl mr-3"
          style={{ backgroundColor: chain.color }}
        >
          {chain.icon}
        </span>
        <div>
          <h1 className="text-2xl font-bold text-gray-900">
            {chain.name} {t('transfer.title')}
          </h1>
          <p className="text-sm text-gray-500">{t('chains.sendTokens', { chain: chain.name })}</p>
        </div>
      </div>

      {error && (
        <div className="mb-4 p-3 bg-red-50 border border-red-200 rounded-lg text-red-700 flex items-center">
          <AlertCircle className="w-5 h-5 mr-2" />
          {error}
        </div>
      )}

      {success && (
        <div className="mb-4 p-3 bg-green-50 border border-green-200 rounded-lg text-green-700 flex items-center">
          <CheckCircle className="w-5 h-5 mr-2" />
          {success}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
        {/* Transfer Form */}
        <Card title={t('transfer.newTransfer')}>
          {!activeWallet ? (
            <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-yellow-800">
              <p>{t('transfer.noActiveWallet')}</p>
            </div>
          ) : (
            <form onSubmit={handleInitiate} className="space-y-4">
              {/* Wallet Info with Balance */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {t('transfer.fromWallet')}
                </label>
                <div className="p-3 bg-gray-50 rounded-lg">
                  <div className="flex justify-between items-start">
                    <div>
                      <p className="font-medium">{activeWallet.name}</p>
                      <p className="text-sm text-gray-500 font-mono">
                        {activeWallet.address}
                      </p>
                    </div>
                    <Wallet className="w-5 h-5 text-gray-400" />
                  </div>

                  {/* Balance Display */}
                  <div className="mt-3 pt-3 border-t border-gray-200">
                    <div className="flex items-center justify-between text-sm">
                      <span className="text-gray-500">{t('transfer.currentBalance')}:</span>
                      {isLoadingBalance ? (
                        <LoadingSpinner size="sm" />
                      ) : walletBalance ? (
                        <div className="text-right">
                          <p className="font-semibold text-gray-900">
                            {parseFloat(walletBalance.native_balance).toFixed(6)} {chain.symbol}
                          </p>
                          {walletBalance.tokens.length > 0 && (
                            <div className="flex flex-wrap gap-1 justify-end mt-1">
                              {walletBalance.tokens.map((t) => (
                                <span key={t.symbol} className="text-xs text-gray-600 bg-gray-200 px-1.5 py-0.5 rounded">
                                  {parseFloat(t.balance).toFixed(2)} {t.symbol}
                                </span>
                              ))}
                            </div>
                          )}
                        </div>
                      ) : (
                        <span className="text-gray-400">--</span>
                      )}
                    </div>
                  </div>
                </div>
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {t('transfer.toAddress')}
                </label>
                <input
                  type="text"
                  value={toAddress}
                  onChange={(e) => setToAddress(e.target.value)}
                  className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono"
                  placeholder={chain.addressPrefix + '...'}
                  required
                />
              </div>

              {/* Token Selector */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {t('transfer.selectToken')}
                </label>
                <div className="grid grid-cols-3 gap-2">
                  {chain.tokens.map((tk) => (
                    <button
                      key={tk.symbol}
                      type="button"
                      onClick={() => setToken(tk.symbol)}
                      disabled={pendingTransfer !== null}
                      className={`flex items-center justify-center px-3 py-2 border rounded-lg transition-colors ${
                        token === tk.symbol
                          ? 'border-blue-500 bg-blue-50 text-blue-700'
                          : 'border-gray-300 hover:border-gray-400'
                      } ${pendingTransfer !== null ? 'opacity-50 cursor-not-allowed' : ''}`}
                    >
                      <span
                        className="w-5 h-5 rounded-full flex items-center justify-center text-white text-xs mr-2"
                        style={{ backgroundColor: tk.color }}
                      >
                        {tk.icon}
                      </span>
                      <span className="font-medium">{tk.symbol}</span>
                    </button>
                  ))}
                </div>
              </div>

              {/* Amount Input */}
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-1">
                  {t('transfer.amount')}
                  {walletBalance && (
                    <span className="ml-2 text-xs text-gray-400">
                      ({t('transfer.available')}: {parseFloat(getCurrentTokenBalance()).toFixed(4)} {token})
                    </span>
                  )}
                </label>
                <div className="relative">
                  <input
                    type="text"
                    value={amount}
                    onChange={(e) => setAmount(e.target.value)}
                    className="w-full px-3 py-2 pr-16 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                    placeholder="0.0"
                    required
                  />
                  <button
                    type="button"
                    onClick={() => setAmount(getCurrentTokenBalance())}
                    className="absolute right-2 top-1/2 -translate-y-1/2 px-2 py-1 text-xs text-blue-600 hover:bg-blue-50 rounded"
                  >
                    MAX
                  </button>
                </div>
              </div>

              {/* Gas/Fee Settings - Only for EVM chains */}
              {isEVM && (
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-1">
                      {t('transfer.gasPrice')}
                    </label>
                    <input
                      type="text"
                      value={gasPriceGwei}
                      onChange={(e) => setGasPriceGwei(e.target.value)}
                      className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      placeholder={t('transfer.auto')}
                    />
                  </div>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-1">
                      {t('transfer.gasLimit')}
                    </label>
                    <input
                      type="text"
                      value={gasLimit}
                      onChange={(e) => setGasLimit(e.target.value)}
                      className="w-full px-3 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500"
                      placeholder={t('transfer.auto')}
                    />
                  </div>
                </div>
              )}

              <button
                type="submit"
                disabled={isSubmitting || pendingTransfer !== null}
                className="w-full flex items-center justify-center px-4 py-2 text-white rounded-lg hover:opacity-90 disabled:opacity-50"
                style={{ backgroundColor: chain.color }}
              >
                {isSubmitting ? (
                  <LoadingSpinner size="sm" />
                ) : (
                  <>
                    <ArrowRight className="w-4 h-4 mr-2" />
                    {t('transfer.initiateTransfer')}
                  </>
                )}
              </button>
            </form>
          )}
        </Card>

        {/* Pending Transfer Confirmation */}
        <Card title={t('transfer.confirmation')}>
          {pendingTransfer ? (
            <div className="space-y-4">
              <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg">
                <p className="text-yellow-800 font-medium">
                  {t('transfer.reviewAndConfirm')}
                </p>
              </div>

              {/* Transfer Details */}
              <div className="space-y-3">
                <div className="flex justify-between">
                  <span className="text-gray-500">{t('transfer.from')}:</span>
                  <span className="font-mono text-sm">
                    {pendingTransfer.from_address.slice(0, 10)}...
                    {pendingTransfer.from_address.slice(-8)}
                  </span>
                </div>
                <div className="flex justify-between">
                  <span className="text-gray-500">{t('transfer.to')}:</span>
                  <span className="font-mono text-sm">
                    {pendingTransfer.to_address.slice(0, 10)}...
                    {pendingTransfer.to_address.slice(-8)}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-gray-500">{t('transfer.amount')}:</span>
                  <span className="font-semibold">
                    {parseFloat(pendingTransfer.amount).toFixed(6)} {pendingTransfer.token}
                  </span>
                </div>
                <div className="flex justify-between items-center">
                  <span className="text-gray-500">{t('transfer.chain')}:</span>
                  <div className="flex items-center space-x-1 text-sm">
                    <span>{chain.icon}</span>
                    <span>{chain.name}</span>
                  </div>
                </div>
              </div>

              {/* Fee Details */}
              {gasEstimate && (
                <div className="p-4 bg-blue-50 border border-blue-200 rounded-lg space-y-3">
                  <div className="flex items-center text-blue-800 font-medium">
                    <Fuel className="w-4 h-4 mr-2" />
                    {t('transfer.feeDetails')} {isEVM && '(EIP-1559)'}
                  </div>

                  <div className="space-y-2 text-sm">
                    {isEVM && (
                      <>
                        <div className="flex justify-between">
                          <span className="text-gray-600">Gas Limit:</span>
                          <span className="font-mono">{gasEstimate.gas_limit.toLocaleString()}</span>
                        </div>
                        {gasEstimate.base_fee_gwei && (
                          <div className="flex justify-between">
                            <span className="text-gray-600">Base Fee:</span>
                            <span className="font-mono">{parseFloat(gasEstimate.base_fee_gwei).toFixed(4)} Gwei</span>
                          </div>
                        )}
                        {gasEstimate.priority_fee_gwei && (
                          <div className="flex justify-between">
                            <span className="text-gray-600">Priority Fee:</span>
                            <span className="font-mono">{parseFloat(gasEstimate.priority_fee_gwei).toFixed(1)} Gwei</span>
                          </div>
                        )}
                        <div className="border-t border-blue-200 my-2"></div>
                      </>
                    )}
                    <div className="flex justify-between">
                      <span className="text-gray-600">{t('transfer.estimatedFee')}:</span>
                      <span className="font-semibold text-orange-600">
                        {parseFloat(gasEstimate.estimated_fee_eth).toFixed(8)} {chain.symbol}
                      </span>
                    </div>
                  </div>
                </div>
              )}

              {/* Balance Summary */}
              {walletBalance && gasEstimate && (
                <div className="p-4 bg-gray-50 border border-gray-200 rounded-lg space-y-3">
                  <div className="flex items-center text-gray-800 font-medium">
                    <Calculator className="w-4 h-4 mr-2" />
                    {t('transfer.balanceSummary')}
                  </div>

                  <div className="space-y-2 text-sm">
                    <div className="flex justify-between">
                      <span className="text-gray-600">{t('transfer.recipientReceives')}:</span>
                      <span className="font-semibold text-green-600">
                        +{parseFloat(amount || '0').toFixed(6)} {token}
                      </span>
                    </div>

                    <div className="flex justify-between">
                      <span className="text-gray-600">{t('transfer.yourBalanceAfter')} ({token}):</span>
                      <span className="font-mono">
                        {getAfterTransferBalance()} {token}
                      </span>
                    </div>

                    {!isNativeToken(chainId, token) && (
                      <div className="flex justify-between">
                        <span className="text-gray-600">{t('transfer.yourBalanceAfter')} ({chain.symbol}):</span>
                        <span className="font-mono">
                          {getNativeAfterFee()} {chain.symbol}
                        </span>
                      </div>
                    )}

                    {isNativeToken(chainId, token) && (
                      <div className="flex justify-between pt-2 border-t border-gray-300">
                        <span className="text-gray-700 font-medium">{t('transfer.totalCost')}:</span>
                        <span className="font-semibold text-red-600">
                          -{(parseFloat(amount || '0') + parseFloat(gasEstimate.estimated_fee_eth)).toFixed(8)} {chain.symbol}
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              )}

              <div className="flex space-x-3">
                <button
                  onClick={resetForm}
                  className="flex-1 px-4 py-2 border border-gray-300 text-gray-700 rounded-lg hover:bg-gray-50"
                >
                  {t('common.cancel')}
                </button>
                <button
                  onClick={handleExecute}
                  disabled={isSubmitting}
                  className="flex-1 px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50"
                >
                  {isSubmitting ? <LoadingSpinner size="sm" /> : t('transfer.executeTransfer')}
                </button>
              </div>
            </div>
          ) : (
            <div className="text-center py-8 text-gray-500">
              <p>{t('transfer.noPending')}</p>
              <p className="text-sm mt-2">
                {t('transfer.fillForm')}
              </p>
            </div>
          )}
        </Card>
      </div>
    </div>
  );
}
