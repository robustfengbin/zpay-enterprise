// Types for F4.1 Orchard → Ironwood migration runs.
// Aligned with backend models_f4.rs (see PRD-F4 docs/2026-07-24/).

export type MigrationRunStatus =
  | 'pending'
  | 'awaiting_approval'
  | 'approved'
  | 'rejected'
  | 'executing'
  | 'partial'
  | 'completed'
  | 'failed'
  | 'canceled';

export type MigrationItemStatus = 'pending' | 'submitted' | 'failed' | 'canceled';

export type MigrationMode = 'immediate' | 'private';

export interface MigrationRun {
  id: number;
  source_wallet_id: number;
  mode: MigrationMode;
  batch_count: number;
  window_hours: number;
  total_amount: string;
  item_count: number;
  status: MigrationRunStatus;
  created_by_user_id: number;
  approved_by_user_id: number | null;
  reject_reason: string | null;
  executed_by_user_id: number | null;
  executed_at: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface MigrationItem {
  id: number;
  run_id: number;
  seq: number;
  amount: string;
  scheduled_at: string | null;
  status: MigrationItemStatus;
  tx_hash: string | null;
  error_message: string | null;
  retry_count: number;
  last_attempt_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface MigrationRunSummary {
  run: MigrationRun;
  items: MigrationItem[];
}

export interface CreateMigrationRunRequest {
  source_wallet_id: number;
  mode: MigrationMode;
  batch_count?: number;
  window_hours?: number;
  notes?: string;
}

export interface MigrationStatus {
  wallet_id: number;
  spendable_zatoshis: number;
  total_zatoshis: number;
  unspent_note_count: number;
  active_run_id: number | null;
  active_run_status: MigrationRunStatus | null;
}

export type ExecuteMigrationOutcome =
  | { result: 'awaiting_approval'; run_id: number; policy_id: number; threshold: string }
  | { result: 'executing'; run_id: number };
