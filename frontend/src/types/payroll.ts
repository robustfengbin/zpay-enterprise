// Types for F3.1 Payroll Run (batch ZEC payroll, no swap)
// Aligned with docs/PRD-F3.1-payroll-run.md

export type PayrollPrivacyMode = 'shielded' | 'direct';

export type PayrollRunStatus =
  | 'draft'
  | 'awaiting_approval'   // shares F2.1 status namespace at the run level
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

export interface Employee {
  id: number;
  name: string;
  wallet_address: string;
  preferred_chain: string;
  preferred_token: string;
  privacy_mode: PayrollPrivacyMode;
  kyc_status: string;
  created_at: string;
}

export interface PayrollRun {
  id: number;
  tenant_id: number;
  pay_period: string | null;
  source_chain: string;
  source_token: string;
  source_amount: string;
  status: PayrollRunStatus;
  created_by: number;
  approved_by: number | null;
  idempotency_key: string | null;
  created_at: string;
  updated_at: string;
  // Aggregate counts for list view
  total_items?: number;
  succeeded_items?: number;
  failed_items?: number;
}

export interface PayrollItem {
  id: number;
  run_id: number;
  employee_id: number;
  employee_name: string;
  target_chain: string;
  target_token: string;
  amount_source: string | null;
  amount_target: string | null;
  privacy_mode: PayrollPrivacyMode;
  status: PayrollItemStatus;
  linked_transfer_ids: number[];
  failure_reason: string | null;
  created_at: string;
}

export interface CreatePayrollRunRequest {
  pay_period?: string;
  source_chain: string;
  source_token: string;
  source_amount: string;
  items: CreatePayrollItem[];
}

export interface CreatePayrollItem {
  // Either reference an existing employee or inline new
  employee_id?: number;
  // Inline employee creation (created on the fly during CSV upload)
  employee_name?: string;
  wallet_address?: string;
  target_chain: string;
  target_token: string;
  amount_source: string;
  privacy_mode: PayrollPrivacyMode;
}

export interface CsvPreviewRow {
  row_index: number;
  employee_name: string;
  wallet_address: string;
  target_chain: string;
  target_token: string;
  amount: string;
  privacy_mode: PayrollPrivacyMode;
  // Pre-validation results
  address_valid: boolean;
  address_chain_match: boolean;
  in_address_book: boolean;
  errors: string[];
}

export interface CsvPreviewResponse {
  total_rows: number;
  valid_rows: number;
  invalid_rows: number;
  rows: CsvPreviewRow[];
}
