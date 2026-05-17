// Types for F1.1 Viewing Key audit + ZIP-307 payment disclosure
// Aligned with docs/PRD-F1.1-viewing-key-audit.md

export type AuditorRole = 'auditor';

/**
 * Disclosure granularity per F1.7 (Selective Disclosure 颗粒控制).
 * Three tiers: single tx / per-address history / time-range full report.
 */
export type DisclosureGranularity = 'single_tx' | 'single_address' | 'time_range';

export type DisclosureFormat = 'pdf' | 'csv' | 'json';

/**
 * Orchard viewing key type selector.
 *   - ovk: Outgoing viewing key — auditor sees outgoing spends only
 *   - ivk: Incoming viewing key — auditor sees incoming notes only
 *   - ufvk: Unified full viewing key — both directions (most powerful)
 */
export type ViewingKeyType = 'ovk' | 'ivk' | 'ufvk';

export interface ViewingKeyExportRequest {
  /** Re-verifies the admin's password so a leaked JWT alone can't export keys. */
  password: string;
  key_type: ViewingKeyType;
}

/**
 * Response from POST /wallets/{id}/viewing-keys/export.
 * The actual key material is NOT returned here — it requires a follow-up
 * GET /viewing-keys/download/{token} which is single-use.
 */
export interface ViewingKeyExportResponse {
  export_id: number;
  /** Base64url, URL-safe ~43 chars. One-time download token, 24h TTL. */
  download_token: string;
  expires_at: string;
}

/**
 * Response from GET /viewing-keys/download/{token}.
 * The backend returns text/plain with a header-prefixed key string of
 * the form `orchard-ufvk:account=0:birthday=2400000:hex=...`. NOT standard
 * ZIP-316; Zashi/Zecwallet won't recognize it directly. Holders parse with
 * Orchard-aware tooling. ZIP-316 emission is M2 polish.
 */
export interface ViewingKeyDownloadResponse {
  /** Raw key string with the orchard-ufvk:... prefix. */
  key_text: string;
}

export interface DisclosureRequest {
  wallet_id: number;
  granularity: DisclosureGranularity;
  // For 'single_tx': required
  tx_hash?: string;
  // For 'single_address': required
  address?: string;
  // For 'time_range': required
  start_at?: string;
  end_at?: string;
  // Output format
  format: DisclosureFormat;
}

export interface DisclosureItem {
  tx_hash: string;
  block_height: number;
  timestamp: string;
  amount_zatoshi: string;
  // For shielded notes — decrypted via viewing key
  recipient_address: string | null;
  memo_decoded: string | null;
}

export interface DisclosureResponse {
  disclosure_id: string;
  wallet_id: number;
  granularity: DisclosureGranularity;
  items: DisclosureItem[];
  // For PDF/CSV: a signed download URL
  download_url: string | null;
  generated_at: string;
  generated_by: number;
}

/**
 * Auditor read-only wallet entry — one row per scoped wallet from
 * GET /auditor/wallets. Carries identity + scope window + disclosure budget
 * + aggregate counters. Raw amounts require a balance / transfers /
 * disclosure call.
 */
export interface AuditorTenantSummary {
  wallet_id: number;
  wallet_name: string;
  chain: string;
  address: string;
  // Scope window (set when admin grants this auditor access)
  scope_start: string;
  scope_end: string;
  // Disclosure budget — how many ZIP-307 payloads this auditor may issue
  max_disclosure_count: number;
  current_count: number;
  // Aggregates over the scope window
  total_tx_count: number;
  last_activity_at: string | null;
  pending_disclosures: number;
}

/**
 * GET /auditor/wallets/{id}/balance.
 *
 * Native balance is always present. For Zcash the value is the combined
 * (transparent + shielded) total; for other chains it is the transparent
 * native balance. ERC-20 / token balances (if any) ride along on `tokens`.
 */
export interface AuditorWalletBalance {
  address: string;
  chain: string;
  native_balance: string;
  tokens: AuditorTokenBalance[];
}

export interface AuditorTokenBalance {
  symbol: string;
  contract: string;
  balance: string;
  decimals: number;
}

/**
 * One transfer row inside the auditor transfers window.
 * Echoes the same Transfer model the admin side sees; fields are
 * intentionally loose because backend Transfer carries many optional
 * approval / payroll linkage fields.
 */
export interface AuditorTransfer {
  id: number;
  wallet_id: number;
  chain: string;
  token: string;
  from_address: string;
  to_address: string;
  amount: string;
  status: string;
  tx_hash: string | null;
  block_number: number | null;
  created_at: string;
  [key: string]: unknown;
}

export interface AuditorTransfersResponse {
  wallet_id: number;
  scope_start: string;
  scope_end: string;
  transfers: AuditorTransfer[];
}
