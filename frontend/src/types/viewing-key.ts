// Types for F1.1 Viewing Key audit + ZIP-307 payment disclosure
// Aligned with docs/PRD-F1.1-viewing-key-audit.md

export type AuditorRole = 'auditor';

/**
 * Disclosure granularity per F1.7 (Selective Disclosure 颗粒控制).
 * Three tiers: single tx / per-address history / time-range full report.
 *
 * Wire values match backend `payment_disclosure_service.rs` (tx | address | range).
 */
export type DisclosureGranularity = 'tx' | 'address' | 'range';

export type DisclosureFormat = 'pdf' | 'csv' | 'json';

export type DisclosureStatus = 'generating' | 'ready' | 'failed';

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

/**
 * scope_param payload — backend validates the keys match the granularity:
 *   tx     → { tx_hash }
 *   address→ { address }
 *   range  → { from, to } where each is either a u64 block height OR an
 *            ISO 8601 timestamp string (backend auto-detects + converts via
 *            ChainClient.block_at_timestamp, per b0793fd).
 */
export type DisclosureScopeParam =
  | { tx_hash: string }
  | { address: string }
  | { from: string | number; to: string | number };

export interface DisclosureRequest {
  granularity: DisclosureGranularity;
  scope_param: DisclosureScopeParam;
  format: DisclosureFormat;
}

/**
 * 202 Accepted from POST /wallets/{id}/payment-disclosures.
 * Body is built asynchronously — poll GET /payment-disclosures/{id} until
 * `status === 'ready'`, then GET /payment-disclosures/{id}/download.
 */
export interface DisclosureCreateResponse {
  disclosure_id: number;
  status: DisclosureStatus;
  tx_count: number;
  created_at: string;
  expires_at: string;
}

/**
 * Full row from GET /payment-disclosures/{id} (mirrors backend `PaymentDisclosure`).
 * `disclosure_json` is the ZIP-307-inspired body; populated only when
 * `status === 'ready'`. Audit / admin both read the same shape.
 */
export interface DisclosureRow {
  id: number;
  wallet_id: number;
  granularity: string;
  scope_param: Record<string, unknown>;
  tx_count: number;
  format: DisclosureFormat;
  status: DisclosureStatus;
  error_message: string | null;
  expires_at: string;
  created_at: string;
  /** Admin-side rows only — the auditor list endpoint returns metadata
      without these (scope-windowed, body stripped). */
  generated_by_user_id?: number;
  disclosure_json?: DisclosureBody | null;
  file_path?: string | null;
}

/**
 * One Orchard note row inside `disclosure_json.actions`. Decrypted via the
 * wallet's receiver IVK during sync — the body is the *result*, not the
 * cryptographic proof (Halo 2 component is M2 scope).
 */
export interface DisclosureAction {
  tx_hash: string;
  block_height: number;
  position_in_block: number;
  value_zatoshis: number;
  value_zec: number;
  memo: string | null;
  nullifier: string | null;
  is_spent: boolean;
  spent_in_tx: string | null;
  recipient_address_hex: string | null;
}

/**
 * The disclosure_json body shape (from backend `build_disclosure_body`).
 * `resolved_range` only present for granularity=range (b0793fd).
 */
export interface DisclosureBody {
  zip_version: string;
  generated_at: string;
  wallet_address: string;
  granularity: DisclosureGranularity;
  format: DisclosureFormat;
  scope: Record<string, unknown>;
  actions: DisclosureAction[];
  action_count: number;
  notes?: string;
  resolved_range?: {
    from_height: number;
    to_height: number;
    from_ts: string;
    to_ts: string;
  };
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
/**
 * Balance response is chain-shaped: Zcash wallets return the combined
 * transparent + shielded shape (get_combined_zcash_balance), other chains
 * return native_balance + ERC-20 tokens. All fields optional accordingly —
 * discriminate on `shielded_balance` presence.
 */
export interface AuditorWalletBalance {
  address: string;
  chain?: string;
  wallet_id?: number;
  native_balance?: string;
  tokens?: AuditorTokenBalance[];
  transparent_balance?: string;
  shielded_balance?: {
    total_zatoshis: number;
    spendable_zatoshis: number;
    pending_zatoshis: number;
  };
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
