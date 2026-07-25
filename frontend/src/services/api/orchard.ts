/**
 * Orchard Privacy Protocol API Service
 *
 * API endpoints for Zcash Orchard shielded transfers.
 *
 * Note: axios interceptor already extracts response.data, so we return
 * the result directly without accessing .data again.
 */

import axios from './axios';
import type {
  CombinedZcashBalance,
  EnableOrchardRequest,
  EnableOrchardResponse,
  ExecuteTransferRequest,
  GenerateAddressRequest,
  OrchardTransactionInfo,
  OrchardTransferProposal,
  OrchardTransferRequest,
  OrchardTransferResponse,
  ScanProgress,
  ShieldedBalance,
  ShieldedBalanceByPool,
  StoredOrchardNote,
  UnifiedAddressInfo,
} from '../../types/orchard';

/**
 * Enable Orchard for a Zcash wallet
 *
 * This derives Orchard keys and generates a unified address.
 */
export async function enableOrchard(
  request: EnableOrchardRequest
): Promise<EnableOrchardResponse> {
  return axios.post(
    `/wallets/${request.wallet_id}/orchard/enable`,
    { birthday_height: request.birthday_height }
  );
}

/**
 * Get all unified addresses for a wallet
 */
export async function getUnifiedAddresses(
  walletId: number
): Promise<UnifiedAddressInfo[]> {
  return axios.get(`/wallets/${walletId}/orchard/addresses`);
}

/**
 * Generate a new unified address
 */
export async function generateUnifiedAddress(
  walletId: number,
  request: GenerateAddressRequest
): Promise<UnifiedAddressInfo> {
  return axios.post(`/wallets/${walletId}/orchard/addresses`, request);
}

/**
 * Get shielded (Orchard) balance for a wallet
 */
export async function getShieldedBalance(
  walletId: number
): Promise<ShieldedBalance> {
  return axios.get(`/wallets/${walletId}/orchard/balance`);
}

/**
 * Get the shielded balance split per pool (legacy Orchard vs Ironwood)
 */
export async function getShieldedBalanceByPool(
  walletId: number
): Promise<ShieldedBalanceByPool> {
  return axios.get(`/wallets/${walletId}/orchard/balance/by-pool`);
}

/**
 * Get combined balance (transparent + shielded)
 */
export async function getCombinedBalance(
  walletId: number
): Promise<CombinedZcashBalance> {
  return axios.get(`/wallets/${walletId}/orchard/balance/combined`);
}

/**
 * Get unspent notes for a wallet
 */
export async function getUnspentNotes(
  walletId: number
): Promise<StoredOrchardNote[]> {
  return axios.get(`/wallets/${walletId}/orchard/notes`);
}

/**
 * Initiate an Orchard (shielded) transfer
 * Returns a transfer proposal with fee estimation
 */
export async function initiateOrchardTransfer(
  request: OrchardTransferRequest
): Promise<OrchardTransferProposal> {
  return axios.post('/transfers/orchard', request);
}

/**
 * Execute a pending Orchard transfer
 * Requires the full proposal data from initiateOrchardTransfer
 */
export async function executeOrchardTransfer(
  proposalId: string,
  request: ExecuteTransferRequest
): Promise<OrchardTransferResponse> {
  return axios.post(`/transfers/orchard/${proposalId}/execute`, request);
}

/**
 * Get Orchard transaction history
 */
export async function getOrchardTransactions(
  walletId: number,
  limit: number = 50
): Promise<OrchardTransactionInfo[]> {
  return axios.get(`/wallets/${walletId}/orchard/transactions`, {
    params: { limit },
  });
}

/**
 * Get scan progress
 */
export async function getScanProgress(): Promise<ScanProgress> {
  return axios.get('/zcash/scan/status');
}

/**
 * Trigger a sync of the Orchard scanner
 */
export async function syncOrchard(): Promise<ScanProgress> {
  return axios.post('/zcash/scan/sync');
}

/**
 * Parse a unified address to get its components
 */
export async function parseUnifiedAddress(
  address: string
): Promise<UnifiedAddressInfo> {
  return axios.get('/zcash/address/parse', { params: { address } });
}

/**
 * Validate a unified address
 */
export async function validateUnifiedAddress(
  address: string
): Promise<{ valid: boolean; error?: string }> {
  try {
    return await axios.get('/zcash/address/validate', { params: { address } });
  } catch (error: any) {
    return {
      valid: false,
      error: error.response?.data?.message || 'Invalid address',
    };
  }
}

/**
 * Estimate fee for an Orchard transfer
 */
export async function estimateOrchardFee(
  request: Omit<OrchardTransferRequest, 'wallet_id'>
): Promise<{
  fee_zatoshis: number;
  fee_zec: string;
  num_actions: number;
}> {
  return axios.post('/transfers/orchard/estimate-fee', request);
}

/**
 * Check if Orchard is enabled for a wallet
 */
export async function isOrchardEnabled(walletId: number): Promise<boolean> {
  try {
    await getShieldedBalance(walletId);
    return true;
  } catch {
    return false;
  }
}

// Export all functions as a namespace for convenient imports
const orchardApi = {
  enableOrchard,
  getUnifiedAddresses,
  generateUnifiedAddress,
  getShieldedBalance,
  getShieldedBalanceByPool,
  getCombinedBalance,
  getUnspentNotes,
  initiateOrchardTransfer,
  executeOrchardTransfer,
  getOrchardTransactions,
  getScanProgress,
  syncOrchard,
  parseUnifiedAddress,
  validateUnifiedAddress,
  estimateOrchardFee,
  isOrchardEnabled,
};

export default orchardApi;
