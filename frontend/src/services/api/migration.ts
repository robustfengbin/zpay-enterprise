// API client for F4.1 Orchard → Ironwood migration runs.
// Aligned with backend handlers/migration.rs + services/migration_service.rs.

import api from './axios';
import type {
  CreateMigrationRunRequest,
  ExecuteMigrationOutcome,
  MigrationRun,
  MigrationRunSummary,
  MigrationStatus,
} from '../../types/migration';
import { generateIdempotencyKey } from './approval';

export const migrationService = {
  /** Wallet banner data source: legacy-pool holdings + any active run. */
  async walletStatus(walletId: number): Promise<MigrationStatus> {
    return api.get(`/wallets/${walletId}/migration-status`);
  },

  async listRuns(limit = 50, offset = 0): Promise<MigrationRun[]> {
    return api.get('/migrations', { params: { limit, offset } });
  },

  async getRun(id: number): Promise<MigrationRunSummary> {
    return api.get(`/migrations/${id}`);
  },

  async createRun(req: CreateMigrationRunRequest): Promise<MigrationRunSummary> {
    return api.post('/migrations', req, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Starts an immediate run or arms a private run's schedule.
   * `{result:"awaiting_approval"}` when the total pivots into F2.1;
   * `{result:"executing"}` once the background executor owns the run.
   * One approval covers the whole window — batches then run unattended
   * and cancel is the only way to stop the remainder.
   */
  async executeRun(id: number): Promise<ExecuteMigrationOutcome> {
    return api.post(`/migrations/${id}/execute`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  async approveRun(id: number): Promise<MigrationRun> {
    return api.post(`/migrations/${id}/approve`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  async rejectRun(id: number, reason: string): Promise<MigrationRun> {
    return api.post(`/migrations/${id}/reject`, { reason });
  },

  async cancelRun(id: number): Promise<MigrationRun> {
    return api.post(`/migrations/${id}/cancel`);
  },

  async retryItem(runId: number, itemId: number): Promise<MigrationRunSummary> {
    return api.post(`/migrations/${runId}/items/${itemId}/retry`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },
};

export function zatToZec(zatoshis: number): string {
  return (zatoshis / 100_000_000).toFixed(8).replace(/\.?0+$/, '') || '0';
}
