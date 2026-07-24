// API client for F4.2 generic batch privacy transfers.
// Aligned with backend handlers/batch_transfer.rs + services/batch_transfer_service.rs.

import api from './axios';
import type {
  BatchTransferRun,
  BatchTransferRunSummary,
  CreateBatchTransferRunRequest,
  CreateBatchTransferRunResponse,
  ExecuteBatchTransferOutcome,
} from '../../types/batch-transfer';
import { generateIdempotencyKey } from './approval';

export const batchTransferService = {
  async listRuns(limit = 50, offset = 0): Promise<BatchTransferRun[]> {
    return api.get('/batch-transfers', { params: { limit, offset } });
  },

  async getRun(id: number): Promise<BatchTransferRunSummary> {
    return api.get(`/batch-transfers/${id}`);
  },

  /**
   * Validates every imported row server-side; a non-empty
   * `validation_errors` means nothing was created (run_id = 0) and the
   * whole CSV can be fixed in one pass.
   */
  async createRun(req: CreateBatchTransferRunRequest): Promise<CreateBatchTransferRunResponse> {
    return api.post('/batch-transfers', req, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Starts the run or arms the staggered schedule. Same F2.1 semantics as
   * migrations: one approval covers the whole window, cancel is the only
   * way to stop remaining items.
   */
  async executeRun(id: number): Promise<ExecuteBatchTransferOutcome> {
    return api.post(`/batch-transfers/${id}/execute`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  async approveRun(id: number): Promise<BatchTransferRun> {
    return api.post(`/batch-transfers/${id}/approve`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  async rejectRun(id: number, reason: string): Promise<BatchTransferRun> {
    return api.post(`/batch-transfers/${id}/reject`, { reason });
  },

  async cancelRun(id: number): Promise<BatchTransferRun> {
    return api.post(`/batch-transfers/${id}/cancel`);
  },

  async retryItem(runId: number, itemId: number): Promise<BatchTransferRunSummary> {
    return api.post(`/batch-transfers/${runId}/items/${itemId}/retry`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },
};
