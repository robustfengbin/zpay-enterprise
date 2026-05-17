// Types for F2.1 Maker/Checker dual-sign transfer
// Aligned with docs/PRD-F2.1-maker-checker.md §4 §5

/**
 * Extended transfer status. The first 4 values are the existing states
 * (kept verbatim from types/index.ts for backward-compat). The next 4
 * are new states introduced by F2.1.
 */
export type TransferStatusExt =
  | 'pending'
  | 'submitted'
  | 'confirmed'
  | 'failed'
  // F2.1 additions:
  | 'awaiting_approval'
  | 'approved'
  | 'rejected'
  | 'expired';

export type ApprovalDecision = 'approve' | 'reject';

export type PolicyScope = 'global' | 'wallet' | 'user';

export interface ApprovalPolicy {
  id: number;
  scope: PolicyScope;
  scope_id: number | null;
  chain: string;
  token: string;
  amount_threshold: string;
  sla_minutes: number;
  required_count: number;
  enabled: boolean;
  created_by: number;
  created_at: string;
  updated_at: string;
}

export interface TransferApproval {
  id: number;
  transfer_id: number;
  approver_user_id: number;
  approver_username: string;
  decision: ApprovalDecision;
  reason: string | null;
  policy_snapshot: ApprovalPolicy | null;
  created_at: string;
}

/**
 * Augments the existing Transfer with F2.1 fields. All new fields are
 * nullable to preserve backward-compatibility per PRD-F2.1 NFR-2.
 */
export interface TransferApprovalFields {
  approval_required: boolean;
  expiry_at: string | null;
  matched_policy_id: number | null;
  approved_at: string | null;
  approved_by: number | null;
  rejection_reason: string | null;
}

export interface PendingApprovalItem {
  transfer_id: number;
  chain: string;
  token: string;
  amount: string;
  from_address: string;
  to_address: string;
  maker_user_id: number;
  maker_username: string;
  memo: string | null;
  matched_policy_id: number | null;
  expiry_at: string;
  created_at: string;
}

export interface PendingApprovalListResponse {
  items: PendingApprovalItem[];
  next_cursor: string | null;
}

export interface ApprovalDecisionRequest {
  // /approve uses { note?: string }; /reject uses { reason: string }
  note?: string;
  reason?: string;
}

export interface ApprovalDecisionResponse {
  transfer_id: number;
  approval_id: number;
  decision: ApprovalDecision;
  approver_user_id: number;
  approver_username: string;
  approved_at: string;
  transfer_status: TransferStatusExt;
}

export interface CreatePolicyRequest {
  scope: PolicyScope;
  scope_id?: number;
  chain: string;
  token: string;
  amount_threshold: string;
  sla_minutes?: number;
  required_count?: number;
}

/**
 * Per-status palette for UI badges. Matches PRD-F2.1 §6.3.
 */
export const TRANSFER_STATUS_PALETTE: Record<TransferStatusExt, { color: string; label_key: string }> = {
  pending:            { color: 'blue',   label_key: 'transfer.status.pending' },
  submitted:          { color: 'blue',   label_key: 'transfer.status.submitted' },
  confirmed:          { color: 'green',  label_key: 'transfer.status.confirmed' },
  failed:             { color: 'red',    label_key: 'transfer.status.failed' },
  awaiting_approval:  { color: 'yellow', label_key: 'approval.status.awaiting' },
  approved:           { color: 'green',  label_key: 'approval.status.approved' },
  rejected:           { color: 'red',    label_key: 'approval.status.rejected' },
  expired:            { color: 'red',    label_key: 'approval.status.expired' },
};
