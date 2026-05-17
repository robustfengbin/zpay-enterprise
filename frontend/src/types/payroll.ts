// Types for F3.1 Payroll Run (batch payroll, single-wallet homogeneous batch)
// Aligned with backend models_m1.rs (see PRD-F3.1 §4).
//
// M1 model: one run = one source wallet (source_wallet_id), chain implied by
// wallet. Per-item target_chain / target_token / privacy_mode is M2 multi-chain
// scope (PRD-F3.1 §4.1).

export type PayrollRunStatus =
  | 'pending'
  | 'awaiting_approval'
  | 'approved'
  | 'rejected'
  | 'executing'
  | 'partial_success'
  | 'completed'
  | 'failed';

export type PayrollItemStatus =
  | 'pending'
  | 'building'
  | 'submitted'
  | 'confirmed'
  | 'failed'
  | 'compensation_pending';

/**
 * Optional UI-only fields stored inside Employee.tags JSON blob.
 * Open shape — M2+ additions land here without ALTER TABLE.
 */
export interface EmployeeTags {
  preferred_token?: string;
  privacy_mode?: 'shielded' | 'direct';
  kyc_status?: 'none' | 'pending' | 'verified';
  [key: string]: unknown;
}

export interface Employee {
  id: number;
  employee_code: string;
  name: string;
  wallet_address: string;
  chain: string;
  tags: EmployeeTags | null;
  active: boolean;
  created_at: string;
  updated_at: string;
}

export interface PayrollRun {
  id: number;
  pay_period: string;
  source_wallet_id: number;
  total_amount: string;
  item_count: number;
  status: PayrollRunStatus;
  created_by_user_id: number;
  executed_by_user_id: number | null;
  executed_at: string | null;
  notes: string | null;
  created_at: string;
  updated_at: string;
}

export interface PayrollItem {
  id: number;
  run_id: number;
  employee_id: number | null;
  employee_address: string;
  amount: string;
  memo: string | null;
  status: PayrollItemStatus;
  tx_hash: string | null;
  block_number: number | null;
  transfer_id: number | null;
  error_message: string | null;
  retry_count: number;
  last_attempt_at: string | null;
  created_at: string;
  updated_at: string;
}

export interface PayrollRunSummary {
  run: PayrollRun;
  items: PayrollItem[];
}

export interface CreatePayrollRunRequest {
  pay_period: string;
  source_wallet_id: number;
  items: PayrollItemInput[];
  notes?: string;
}

export interface PayrollItemInput {
  employee_code?: string;
  employee_address: string;
  amount: string;
  memo?: string;
}

export interface CreatePayrollRunResponse {
  run_id: number;
  item_count: number;
  validation_errors: ValidationError[];
}

export interface ValidationError {
  row_index: number;
  field: string;
  message: string;
}

/**
 * Tagged union from POST /payroll/runs/{id}/execute.
 * Backend uses `#[serde(tag = "result", rename_all = "snake_case")]`.
 */
export type ExecuteRunOutcome =
  | {
      result: 'awaiting_approval';
      run_id: number;
      policy_id: number;
      threshold: string;
    }
  | {
      result: 'executed';
      run_id: number;
      submitted: number;
      failed: number;
      final_status: string;
    };

/**
 * Client-side CSV preview row — backend has no /payroll/csv/preview endpoint
 * (M2 will add it). For now the frontend parses CSV in-browser and shows a
 * preview before POST /payroll/runs.
 */
export interface CsvPreviewRow {
  row_index: number;
  employee_code: string;
  employee_address: string;
  amount: string;
  memo: string;
  errors: string[];
}
