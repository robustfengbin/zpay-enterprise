// Types for F4.2 generic batch privacy transfers.
// Aligned with backend models_f4.rs (see PRD-F4 docs/2026-07-24/ §5).

export type BatchTransferRunStatus =
  | 'pending'
  | 'awaiting_approval'
  | 'approved'
  | 'rejected'
  | 'executing'
  | 'partial'
  | 'completed'
  | 'failed'
  | 'canceled';

export type BatchTransferItemStatus = 'pending' | 'submitted' | 'failed' | 'canceled';

export type PrivacyMode = 'off' | 'staggered';

export interface BatchTransferRun {
  id: number;
  title: string;
  source_wallet_id: number;
  privacy_mode: PrivacyMode;
  batch_count: number;
  window_hours: number;
  total_amount: string;
  item_count: number;
  status: BatchTransferRunStatus;
  created_by_user_id: number;
  approved_by_user_id: number | null;
  reject_reason: string | null;
  executed_by_user_id: number | null;
  executed_at: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface BatchTransferItem {
  id: number;
  run_id: number;
  seq: number;
  recipient_address: string;
  amount: string;
  memo: string | null;
  scheduled_at: string | null;
  status: BatchTransferItemStatus;
  tx_hash: string | null;
  error_message: string | null;
  retry_count: number;
  last_attempt_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface BatchTransferRunSummary {
  run: BatchTransferRun;
  items: BatchTransferItem[];
}

export interface BatchTransferItemInput {
  recipient_address: string;
  amount: string;
  memo?: string;
}

export interface CreateBatchTransferRunRequest {
  title: string;
  source_wallet_id: number;
  privacy_mode: PrivacyMode;
  batch_count?: number;
  window_hours?: number;
  max_per_transfer?: string;
  items: BatchTransferItemInput[];
  notes?: string;
}

export interface BatchValidationError {
  row_index: number;
  field: string;
  message: string;
}

export interface CreateBatchTransferRunResponse {
  run_id: number;
  item_count: number;
  validation_errors: BatchValidationError[];
}

export type ExecuteBatchTransferOutcome =
  | { result: 'awaiting_approval'; run_id: number; policy_id: number; threshold: string }
  | { result: 'executing'; run_id: number };

/** One parsed CSV row with client-side pre-check errors. */
export interface BatchCsvRow {
  row_index: number;
  recipient_address: string;
  amount: string;
  memo: string;
  errors: string[];
}
