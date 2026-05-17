/**
 * `auditor` was added in M1 (F1.1) as a read-only scoped role.
 * Existing 'admin' / 'operator' roles preserved per NFR-2 backward-compat.
 */
export type UserRole = 'admin' | 'operator' | 'auditor';

export interface User {
  id: number;
  username: string;
  role: UserRole;
}

export interface LoginResponse {
  token: string;
  user: User;
}

export interface Wallet {
  id: number;
  name: string;
  address: string;
  chain: string;
  is_active: boolean;
  created_at: string;
}

export interface TokenBalance {
  symbol: string;
  balance: string;
  contract_address: string | null;
}

export interface BalanceResponse {
  address: string;
  chain: string;
  native_balance: string;
  tokens: TokenBalance[];
}

export interface Transfer {
  id: number;
  wallet_id: number;
  chain: string;
  from_address: string;
  to_address: string;
  token: string;
  amount: string;
  gas_price: string | null;
  gas_limit: number | null;
  gas_used: number | null;
  status: 'pending' | 'submitted' | 'confirmed' | 'failed';
  tx_hash: string | null;
  block_number: number | null;
  error_message: string | null;
  initiated_by: number;
  created_at: string;
  updated_at: string;
}

export interface TransferListResponse {
  transfers: Transfer[];
  total: number;
  limit: number;
  offset: number;
}

export interface ChainInfo {
  id: string;
  name: string;
  native_token: string;
}

export interface CreateWalletRequest {
  name: string;
  chain?: string;
}

export interface ImportWalletRequest {
  name: string;
  private_key: string;
  chain?: string;
}

export interface TransferRequest {
  chain: string;
  to_address: string;
  token: string;
  amount: string;
  gas_price_gwei?: string;
  gas_limit?: number;
}

export interface ExportPrivateKeyResponse {
  private_key: string;
  warning: string;
}
