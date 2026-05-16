// API client for F1.1 Viewing Key audit + ZIP-307 disclosure
// Aligned with docs/PRD-F1.1-viewing-key-audit.md
// Route shapes reconciled with france's backend skeleton (2026-05-16):
//   POST/GET /api/v1/wallets/{id}/viewing-keys/exports     (Admin side)
//   POST/GET /api/v1/wallets/{id}/payment-disclosures      (Admin side)
//   POST/GET /api/v1/auditor/login + /me + /wallets        (Auditor side)
//   GET      /api/v1/auditor/wallets/{id}/balance + transfers + disclosures

import api from './axios';
import type {
  AuditorTenantSummary,
  DisclosureRequest,
  DisclosureResponse,
  ViewingKeyExportRequest,
  ViewingKeyExportResponse,
} from '../../types/viewing-key';

export const viewingKeyService = {
  // ===========================================================
  // Admin side — manage viewing keys + generate disclosures
  // ===========================================================

  /**
   * POST /wallets/{id}/viewing-keys/exports — Admin only.
   * Creates a new viewing-key export record (auditable).
   */
  async exportViewingKey(req: ViewingKeyExportRequest): Promise<ViewingKeyExportResponse> {
    return api.post(`/wallets/${req.wallet_id}/viewing-keys/exports`, req);
  },

  /**
   * GET /wallets/{id}/viewing-keys/exports — list past exports.
   */
  async listExports(walletId: number): Promise<ViewingKeyExportResponse[]> {
    return api.get(`/wallets/${walletId}/viewing-keys/exports`);
  },

  /**
   * POST /wallets/{id}/payment-disclosures — Admin only.
   * Generates a ZIP-307 payment disclosure report for the wallet.
   */
  async generateDisclosure(req: DisclosureRequest): Promise<DisclosureResponse> {
    return api.post(`/wallets/${req.wallet_id}/payment-disclosures`, req);
  },

  /**
   * GET /wallets/{id}/payment-disclosures — list past disclosures for a wallet.
   */
  async listDisclosures(walletId: number): Promise<DisclosureResponse[]> {
    return api.get(`/wallets/${walletId}/payment-disclosures`);
  },

  // ===========================================================
  // Auditor side — read-only, scoped JWT (separate auth surface)
  // ===========================================================

  /**
   * GET /auditor/wallets — list wallets this auditor is scoped to.
   * Replaces the earlier /auditor/tenants endpoint.
   */
  async listAuditorWallets(): Promise<AuditorTenantSummary[]> {
    return api.get('/auditor/wallets');
  },

  /**
   * GET /auditor/disclosures/{id} — auditor reads a specific disclosure.
   */
  async getDisclosure(disclosureId: string): Promise<DisclosureResponse> {
    return api.get(`/auditor/disclosures/${disclosureId}`);
  },

  /**
   * GET /auditor/wallets/{id}/transfers — auditor reads scoped wallet transfers.
   */
  async getAuditorWalletTransfers(walletId: number): Promise<unknown[]> {
    return api.get(`/auditor/wallets/${walletId}/transfers`);
  },

  /**
   * GET /auditor/wallets/{id}/balance — auditor reads scoped balance aggregate.
   */
  async getAuditorWalletBalance(walletId: number): Promise<unknown> {
    return api.get(`/auditor/wallets/${walletId}/balance`);
  },
};

// ===========================================================
// Auditor authentication (separate from main /auth)
// ===========================================================

export const auditorAuthService = {
  /**
   * POST /auditor/login — issues an auditor-scoped JWT.
   */
  async login(username: string, password: string): Promise<{ token: string; auditor: { id: number; username: string } }> {
    return api.post('/auditor/login', { username, password });
  },

  /**
   * GET /auditor/me — current auditor session info.
   */
  async me(): Promise<{ id: number; username: string; scoped_wallets: number[] }> {
    return api.get('/auditor/me');
  },
};
