/**
 * Orchard Privacy Protocol Types
 *
 * Types for Zcash Orchard shielded transfers and unified addresses.
 */

/** Shielded pool types */
export type ShieldedPool = 'orchard' | 'ironwood' | 'sapling';

/** Unified address information */
export interface UnifiedAddressInfo {
  /** The unified address string (u1...) */
  address: string;
  /** Whether it contains an Orchard receiver */
  has_orchard: boolean;
  /** Whether it contains a Sapling receiver */
  has_sapling: boolean;
  /** Whether it contains a transparent receiver */
  has_transparent: boolean;
  /** The transparent address component (if present) */
  transparent_address: string | null;
  /** Address index in the HD derivation path */
  address_index: number;
  /** The account this address belongs to */
  account_index: number;
}

/** Shielded balance breakdown */
/**
 * Shielded balance split per pool (F4.0-c).
 *
 * A wallet mid-migration holds a draining legacy Orchard balance and a filling
 * Ironwood one; the headline figure is their sum.
 */
export interface ShieldedBalanceByPool {
  wallet_id: number;
  /** One entry per pool, zeroes included, old pool first. */
  pools: ShieldedBalance[];
  /** Sum across pools. */
  total_zatoshis: number;
}

export interface ShieldedBalance {
  /** Total balance in zatoshis */
  total_zatoshis: number;
  /** Spendable balance (confirmed) in zatoshis */
  spendable_zatoshis: number;
  /** Pending balance (unconfirmed) in zatoshis */
  pending_zatoshis: number;
  /** Number of unspent notes */
  note_count: number;
  /** Pool this balance belongs to; absent when the figure is the cross-pool total. */
  pool?: ShieldedPool;
}

/** Combined Zcash balance (transparent + shielded) */
export interface CombinedZcashBalance {
  wallet_id: number;
  address: string;
  /** Transparent balance as string */
  transparent_balance: string;
  /** Shielded balance (null if Orchard not enabled) */
  shielded_balance: ShieldedBalance | null;
  /** Total balance in ZEC */
  total_zec: number;
}

/** Scan progress information */
export interface ScanProgress {
  /** Chain being scanned */
  chain: string;
  /** Type of scan (orchard, sapling) */
  scan_type: string;
  /** Last fully scanned block height */
  last_scanned_height: number;
  /** Current chain tip height */
  chain_tip_height: number;
  /** Percentage complete (0-100) */
  progress_percent: number;
  /** Estimated time remaining in seconds */
  estimated_seconds_remaining: number | null;
  /** Whether scanning is currently active */
  is_scanning: boolean;
  /** Number of notes found */
  notes_found: number;
}

/** Orchard note (received shielded funds) */
export interface OrchardNote {
  id: number | null;
  account_id: number;
  tx_hash: string;
  block_height: number;
  note_commitment: string;
  nullifier: string;
  value_zatoshis: number;
  position: number;
  is_spent: boolean;
  memo: string | null;
}

/** Stored Orchard note from database */
export interface StoredOrchardNote {
  id: number;
  nullifier: string;
  value_zatoshis: number;
  value_zec: number;
  block_height: number;
  tx_hash: string;
  is_spent: boolean;
  memo: string | null;
}

/** Orchard transaction info */
export interface OrchardTransactionInfo {
  tx_hash: string;
  block_height: number;
  value_zatoshis: number;
  is_incoming: boolean;
  memo: string | null;
  pool: ShieldedPool;
}

/** Request to enable Orchard for a wallet */
export interface EnableOrchardRequest {
  wallet_id: number;
  /** Block height when wallet was created */
  birthday_height: number;
}

/** Response from enabling Orchard */
export interface EnableOrchardResponse {
  unified_address: UnifiedAddressInfo;
  viewing_key: string;
}

/** Fund source for transfers */
export type FundSource = 'auto' | 'shielded' | 'transparent';

/** Request for Orchard transfer */
export interface OrchardTransferRequest {
  wallet_id: number;
  to_address: string;
  /** Amount in ZEC */
  amount: string;
  /** Amount in zatoshis */
  amount_zatoshis?: number;
  /** Optional encrypted memo (max 512 characters) */
  memo?: string;
  /** Target pool for the transfer */
  target_pool?: ShieldedPool;
  /** Source of funds: auto (shielded first), shielded only, or transparent only */
  fund_source?: FundSource;
}

/** Response from initiating Orchard transfer (proposal) */
export interface OrchardTransferProposal {
  proposal_id: string;
  amount_zatoshis: number;
  amount_zec: number;
  fee_zatoshis: number;
  fee_zec: number;
  fund_source: string;
  is_shielding: boolean;
  is_deshielding: boolean;
  to_address: string;
  memo?: string;
  expiry_height: number;
}

/** Request to execute a transfer */
export interface ExecuteTransferRequest {
  wallet_id: number;
  proposal_id: string;
  amount_zatoshis: number;
  fee_zatoshis: number;
  to_address: string;
  memo?: string;
  fund_source: string;
  is_shielding: boolean;
  is_deshielding: boolean;
  expiry_height: number;
}

/** Response from executing Orchard transfer */
export interface OrchardTransferResponse {
  tx_id: string;
  status: string;
  raw_tx?: string;
  amount_zatoshis: number;
  fee_zatoshis: number;
}

/** Request to generate new unified address */
export interface GenerateAddressRequest {
  viewing_key: string;
  address_index: number;
}

/** Address type for UI selection */
export type AddressType = 'transparent' | 'unified' | 'orchard_only';

/** Address type configuration */
export interface AddressTypeConfig {
  type: AddressType;
  label: string;
  description: string;
  prefix: string;
  privacyLevel: 'none' | 'partial' | 'full';
}

/** Balance display mode */
export type BalanceDisplayMode = 'combined' | 'separate';

/** Helper functions */
export function zatoshisToZec(zatoshis: number): number {
  return zatoshis / 100_000_000;
}

export function zecToZatoshis(zec: number): number {
  return Math.round(zec * 100_000_000);
}

export function formatZec(zatoshis: number): string {
  const zec = zatoshisToZec(zatoshis);
  return zec.toFixed(8);
}

export function isUnifiedAddress(address: string): boolean {
  return address.startsWith('u1') && address.length >= 100;
}

export function isTransparentAddress(address: string): boolean {
  return (address.startsWith('t1') || address.startsWith('t3')) &&
         address.length >= 34 &&
         address.length <= 36;
}

export function isSaplingAddress(address: string): boolean {
  return address.startsWith('zs') && address.length >= 78;
}

export function getAddressType(address: string): AddressType | 'sapling' | 'unknown' {
  if (isUnifiedAddress(address)) return 'unified';
  if (isTransparentAddress(address)) return 'transparent';
  if (isSaplingAddress(address)) return 'sapling';
  return 'unknown';
}

/** Privacy level descriptions */
export const PRIVACY_LEVELS = {
  none: 'No privacy - all transaction details visible on blockchain',
  partial: 'Partial privacy - recipient can see memo, amounts hidden',
  full: 'Full privacy - all details encrypted and hidden',
} as const;

/** Default address type configurations */
export const ADDRESS_TYPE_CONFIGS: AddressTypeConfig[] = [
  {
    type: 'unified',
    label: 'Unified Address',
    description: 'Receives to all pools (recommended)',
    prefix: 'u1',
    privacyLevel: 'full',
  },
  {
    type: 'transparent',
    label: 'Transparent Address',
    description: 'Like Bitcoin - all details public',
    prefix: 't1',
    privacyLevel: 'none',
  },
  {
    type: 'orchard_only',
    label: 'Orchard Only',
    description: 'Maximum privacy - only Orchard pool',
    prefix: 'u1',
    privacyLevel: 'full',
  },
];
