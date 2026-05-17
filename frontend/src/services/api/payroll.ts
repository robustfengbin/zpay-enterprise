// API client for F3.1 Payroll Run (single-wallet homogeneous batch, M1).
// Aligned with backend handlers/m1.rs + services/payroll_service.rs (5664717).

import api from './axios';
import type {
  CreatePayrollRunRequest,
  CreatePayrollRunResponse,
  Employee,
  ExecuteRunOutcome,
  PayrollItem,
  PayrollRun,
  PayrollRunSummary,
} from '../../types/payroll';
import { generateIdempotencyKey } from './approval';

export const payrollService = {
  // ---------------- Employees ----------------
  async listEmployees(): Promise<Employee[]> {
    return api.get('/payroll/employees');
  },

  async createEmployee(emp: Omit<Employee, 'id' | 'created_at' | 'updated_at'>): Promise<Employee> {
    return api.post('/payroll/employees', emp);
  },

  async updateEmployee(id: number, patch: Partial<Employee>): Promise<Employee> {
    return api.put(`/payroll/employees/${id}`, patch);
  },

  async deleteEmployee(id: number): Promise<{ ok: boolean }> {
    return api.delete(`/payroll/employees/${id}`);
  },

  // ---------------- Payroll runs ----------------
  async listRuns(limit = 20, offset = 0): Promise<PayrollRun[]> {
    return api.get('/payroll/runs', { params: { limit, offset } });
  },

  async getRun(id: number): Promise<PayrollRunSummary> {
    return api.get(`/payroll/runs/${id}`);
  },

  /**
   * 201 on success with empty validation_errors.
   * 422 when any item fails validation — body still has run_id=0 + item_count=0
   * + validation_errors[]. Axios will throw for 422 by default; callers should
   * inspect err.response.data for the CreatePayrollRunResponse payload.
   */
  async createRun(req: CreatePayrollRunRequest): Promise<CreatePayrollRunResponse> {
    return api.post('/payroll/runs', req, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Returns either `{result:"awaiting_approval", policy_id, threshold}` if the
   * run total ≥ a matching policy threshold (F2.1 hook), or
   * `{result:"executed", submitted, failed, final_status}` otherwise.
   */
  async executeRun(id: number): Promise<ExecuteRunOutcome> {
    return api.post(`/payroll/runs/${id}/execute`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Per-item retry. Bulk "retry all failed" fans out N single-item retries
   * client-side until backend exposes a batch endpoint.
   */
  async retryItem(runId: number, itemId: number): Promise<PayrollItem> {
    return api.post(`/payroll/runs/${runId}/items/${itemId}/retry`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Cancel a pending or awaiting_approval run.
   */
  async cancelRun(id: number): Promise<PayrollRun> {
    return api.post(`/payroll/runs/${id}/cancel`);
  },

  /**
   * Run report: { run, items, counts: {...} }. Used for after-action review
   * + future CSV/PDF export (format param reserved for M2).
   */
  async runReport(id: number): Promise<unknown> {
    return api.get(`/payroll/runs/${id}/report`);
  },
};
