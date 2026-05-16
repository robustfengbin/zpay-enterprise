// API client for F3.1 Payroll Run (batch ZEC payroll, no swap)
// Aligned with docs/PRD-F3.1-payroll-run.md

import api from './axios';
import type {
  CreatePayrollRunRequest,
  CsvPreviewResponse,
  Employee,
  PayrollItem,
  PayrollRun,
} from '../../types/payroll';
import { generateIdempotencyKey } from './approval';

export const payrollService = {
  /**
   * Employees CRUD (lightweight — F3.2 will deepen).
   * Route under /payroll/employees per france's backend routing (2026-05-16).
   */
  async listEmployees(): Promise<Employee[]> {
    return api.get('/payroll/employees');
  },

  async createEmployee(emp: Omit<Employee, 'id' | 'created_at'>): Promise<Employee> {
    return api.post('/payroll/employees', emp);
  },

  async updateEmployee(id: number, patch: Partial<Employee>): Promise<Employee> {
    return api.put(`/payroll/employees/${id}`, patch);
  },

  async deleteEmployee(id: number): Promise<{ ok: boolean }> {
    return api.delete(`/payroll/employees/${id}`);
  },

  /**
   * Payroll runs.
   */
  async listRuns(limit = 20, offset = 0): Promise<{ runs: PayrollRun[]; total: number }> {
    return api.get('/payroll/runs', { params: { limit, offset } });
  },

  async getRun(id: number): Promise<PayrollRun & { items: PayrollItem[] }> {
    return api.get(`/payroll/runs/${id}`);
  },

  async createRun(req: CreatePayrollRunRequest): Promise<PayrollRun> {
    return api.post('/payroll/runs', req, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Get a quote for the whole batch (per PRD-F3.1 §5 FR-8 sibling).
   * For pure-ZEC payroll (M1 scope), this estimates fees + Halo 2 proof
   * timing; no swap quote since no swap.
   */
  async quoteRun(id: number): Promise<{ run_id: number; estimated_fee: string; estimated_proof_seconds: number }> {
    return api.post(`/payroll/runs/${id}/quote`);
  },

  /**
   * Execute the batch (maker/checker-approved run).
   */
  async executeRun(id: number): Promise<PayrollRun> {
    return api.post(`/payroll/runs/${id}/execute`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Retry a single failed item per france's route shape:
   * POST /payroll/runs/{run_id}/items/{item_id}/retry
   * (The bulk "retry all failed" is left as a frontend convenience that
   *  fans out N single-item retries until the backend exposes a batch endpoint.)
   */
  async retryItem(runId: number, itemId: number): Promise<{ retried: boolean }> {
    return api.post(`/payroll/runs/${runId}/items/${itemId}/retry`, null, {
      headers: { 'Idempotency-Key': generateIdempotencyKey() },
    });
  },

  /**
   * Cancel a draft / awaiting_approval / approved (pre-execute) run.
   */
  async cancelRun(id: number): Promise<PayrollRun> {
    return api.post(`/payroll/runs/${id}/cancel`);
  },

  /**
   * Run report (CSV/PDF download URL).
   */
  async runReport(id: number, format: 'csv' | 'pdf' = 'csv'): Promise<{ download_url: string }> {
    return api.get(`/payroll/runs/${id}/report`, { params: { format } });
  },

  /**
   * CSV upload preview — validates addresses + chain types pre-submit.
   * Not yet in backend's exposed routes; frontend stub may receive 501 until
   * F3.9 (batch address validation tool) is wired.
   */
  async previewCsv(file: File): Promise<CsvPreviewResponse> {
    const form = new FormData();
    form.append('file', file);
    return api.post('/payroll/csv/preview', form, {
      headers: { 'Content-Type': 'multipart/form-data' },
    });
  },
};
