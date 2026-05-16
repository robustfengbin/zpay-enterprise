// API client for F2.1 Maker/Checker dual-sign transfer
// Aligned with docs/PRD-F2.1-maker-checker.md §5

import api from './axios';
import type {
  ApprovalDecisionRequest,
  ApprovalDecisionResponse,
  ApprovalPolicy,
  CreatePolicyRequest,
  PendingApprovalListResponse,
  TransferApproval,
} from '../../types/approval';

/**
 * Generate a UUID-based idempotency key for write requests.
 * Used in PRD-F2.1 NFR-4.
 */
export function generateIdempotencyKey(): string {
  // Crypto.randomUUID is available in modern browsers (Chrome 92+, FF 95+)
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  // Fallback for older browsers
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export const approvalService = {
  /**
   * GET /transfers/approvals/pending — list transfers I can approve.
   * Backend filters out transfers where I am the maker.
   * Route shape aligned with france's actual handler routing (2026-05-16).
   */
  async listPending(limit = 20, cursor?: string): Promise<PendingApprovalListResponse> {
    const params: Record<string, string | number> = { limit };
    if (cursor) params.cursor = cursor;
    return api.get('/transfers/approvals/pending', { params });
  },

  /**
   * POST /transfers/{id}/approve
   */
  async approve(transferId: number, note?: string): Promise<ApprovalDecisionResponse> {
    const body: ApprovalDecisionRequest = note ? { note } : {};
    return api.post(`/transfers/${transferId}/approve`, body, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * POST /transfers/{id}/reject — reason is required (>= 5 chars)
   */
  async reject(transferId: number, reason: string): Promise<ApprovalDecisionResponse> {
    if (reason.length < 5) {
      throw new Error('Rejection reason must be at least 5 characters');
    }
    return api.post(`/transfers/${transferId}/reject`, { reason } as ApprovalDecisionRequest, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * GET approval history for a single transfer (audit trail).
   */
  async getHistory(transferId: number): Promise<TransferApproval[]> {
    return api.get(`/transfers/${transferId}/approvals`);
  },

  /**
   * DELETE /transfers/{id} — maker self-recall (only in AwaitingApproval state)
   * Per PRD-F2.1 FR-13.
   */
  async recall(transferId: number): Promise<{ ok: boolean }> {
    return api.delete(`/transfers/${transferId}`);
  },

  /**
   * GET /approval-policies — list all policies (Admin only).
   * Route shape aligned with france's actual handler routing (2026-05-16).
   */
  async listPolicies(): Promise<ApprovalPolicy[]> {
    return api.get('/approval-policies');
  },

  /**
   * POST /approval-policies — Admin only
   */
  async createPolicy(req: CreatePolicyRequest): Promise<ApprovalPolicy> {
    return api.post('/approval-policies', req);
  },

  /**
   * PUT /approval-policies/{id} — Admin only
   */
  async updatePolicy(id: number, req: Partial<CreatePolicyRequest>): Promise<ApprovalPolicy> {
    return api.put(`/approval-policies/${id}`, req);
  },

  /**
   * DELETE /approval-policies/{id} — Admin only
   */
  async deletePolicy(id: number): Promise<{ ok: boolean }> {
    return api.delete(`/approval-policies/${id}`);
  },
};
