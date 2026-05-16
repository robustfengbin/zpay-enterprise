// Types for F1.1 Viewing Key audit + ZIP-307 payment disclosure
// Aligned with docs/PRD-F1.1-viewing-key-audit.md

export type AuditorRole = 'auditor';

/**
 * Disclosure granularity per F1.7 (Selective Disclosure 颗粒控制).
 * Three tiers: single tx / per-address history / time-range full report.
 */
export type DisclosureGranularity = 'single_tx' | 'single_address' | 'time_range';

export type DisclosureFormat = 'pdf' | 'csv' | 'json';

export interface ViewingKeyExportRequest {
  wallet_id: number;
  // Standard ZIP-316 unified full viewing key encoding. The internal
  // custom "ufvk:account:birthday:hex" encoding is also available via
  // include_legacy=true for round-trip into the existing platform.
  include_legacy?: boolean;
}

export interface ViewingKeyExportResponse {
  wallet_id: number;
  ufvk_standard: string;       // ZIP-316 UFVK string (Zashi-compatible)
  ufvk_legacy?: string;        // Internal custom encoding (optional)
  birthday_height: number;
  exported_at: string;
  exported_by: number;
  warning: string;
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
 * Auditor read-only summary view of a tenant (per PRD-F1.1 §2).
 */
export interface AuditorTenantSummary {
  wallet_id: number;
  wallet_name: string;
  chain: string;
  address: string;
  // Aggregates only, not raw amounts — Auditor needs disclosure to decrypt
  total_tx_count: number;
  last_activity_at: string | null;
  pending_disclosures: number;
}
