// API client for F1.1 Viewing Key audit + ZIP-307 disclosure
// Aligned with docs/PRD-F1.1-viewing-key-audit.md
// Route shapes reconciled with backend skeleton (see PRD-F1.1 §5):
//   POST/GET /api/v1/wallets/{id}/viewing-keys/exports     (Admin side)
//   POST/GET /api/v1/wallets/{id}/payment-disclosures      (Admin side)
//   POST/GET /api/v1/auditor/login + /me + /wallets        (Auditor side)
//   GET      /api/v1/auditor/wallets/{id}/balance + transfers + disclosures

import api, { auditorApi } from './axios';
import type {
  AuditorTenantSummary,
  AuditorTransfersResponse,
  AuditorWalletBalance,
  DisclosureBody,
  DisclosureCreateResponse,
  DisclosureRequest,
  DisclosureRow,
  ViewingKeyExportRequest,
  ViewingKeyExportResponse,
  ViewingKeyDownloadResponse,
} from '../../types/viewing-key';

export const viewingKeyService = {
  // ===========================================================
  // Admin side — manage viewing keys + generate disclosures
  // ===========================================================

  /**
   * POST /wallets/{id}/viewing-keys/export — Admin only.
   * Creates a new viewing-key export record (auditable). Requires
   * password re-verify (defense-in-depth against leaked JWTs).
   * Returns a one-time download_token; the actual key material is
   * fetched via downloadKey() below and the row is then zeroed.
   */
  async exportViewingKey(walletId: number, req: ViewingKeyExportRequest): Promise<ViewingKeyExportResponse> {
    return api.post(`/wallets/${walletId}/viewing-keys/export`, req);
  },

  /**
   * GET /viewing-keys/download/{token} — single-use, returns text/plain.
   * The backend response header is text/plain, so we hit a different
   * shape: axios returns the raw string body (our axios interceptor
   * returns response.data; for text/plain that is the body string).
   * After this call the token is invalidated server-side.
   */
  async downloadKey(token: string): Promise<ViewingKeyDownloadResponse> {
    const text = await api.get(`/viewing-keys/download/${token}`, {
      // override the default JSON accept; the response is text/plain
      headers: { Accept: 'text/plain' },
      responseType: 'text',
      transformResponse: [(d: unknown) => d as string],
    });
    return { key_text: text as unknown as string };
  },

  /**
   * GET /wallets/{id}/viewing-keys — list past export audit rows.
   * Note: the actual key material is zeroed after download; this surface
   * is purely audit metadata (who exported what, when, downloaded yet).
   */
  async listExports(walletId: number): Promise<ViewingKeyExportResponse[]> {
    return api.get(`/wallets/${walletId}/viewing-keys`);
  },

  /**
   * POST /wallets/{id}/payment-disclosures — Admin only.
   *
   * Backend is async: returns 202 with a stub row whose `status:"generating"`,
   * then a `tokio::spawn` builds the real body. Caller must poll
   * getDisclosure() until `status === 'ready'`.
   */
  async generateDisclosure(walletId: number, req: DisclosureRequest): Promise<DisclosureCreateResponse> {
    return api.post(`/wallets/${walletId}/payment-disclosures`, req);
  },

  /**
   * GET /payment-disclosures/{id} — full row with status. Body still null
   * while generating; populated once status flips to 'ready'.
   */
  async getDisclosure(disclosureId: number): Promise<DisclosureRow> {
    return api.get(`/payment-disclosures/${disclosureId}`);
  },

  /**
   * GET /payment-disclosures/{id}/download — only valid once status='ready'.
   * Returns the disclosure_json body (zip_version="307-enterprise" with
   * actions[] + resolved_range if granularity=range).
   */
  async downloadDisclosure(disclosureId: number): Promise<DisclosureBody> {
    return api.get(`/payment-disclosures/${disclosureId}/download`);
  },

  /**
   * GET /wallets/{id}/payment-disclosures — list past disclosures for a wallet.
   */
  async listDisclosures(walletId: number): Promise<DisclosureRow[]> {
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
    return auditorApi.get('/auditor/wallets');
  },

  /**
   * GET /auditor/wallets/{id}/transfers — auditor reads scoped wallet transfers.
   * Backend bounds the query to the scope's half-open [start, end) window.
   * limit clamps to 1..200 server-side; offset is paged.
   */
  async getAuditorWalletTransfers(
    walletId: number,
    limit = 50,
    offset = 0,
  ): Promise<AuditorTransfersResponse> {
    return auditorApi.get(`/auditor/wallets/${walletId}/transfers`, { params: { limit, offset } });
  },

  /**
   * GET /auditor/wallets/{id}/balance — real on-chain balance for the scoped
   * wallet. Zcash returns combined transparent + shielded; other chains
   * return native + ERC-20 token balances.
   */
  async getAuditorWalletBalance(walletId: number): Promise<AuditorWalletBalance> {
    return auditorApi.get(`/auditor/wallets/${walletId}/balance`);
  },

  /**
   * GET /auditor/wallets/{id}/disclosures — disclosures generated for a
   * wallet in this auditor's scope. Same row shape as the admin-side list.
   */
  async listAuditorWalletDisclosures(walletId: number): Promise<DisclosureRow[]> {
    return auditorApi.get(`/auditor/wallets/${walletId}/disclosures`);
  },
};

// ===========================================================
// Auditor authentication (separate from main /auth)
// ===========================================================

export interface AuditorSession {
  id: number;
  email: string;
  name: string;
  scope_start: string | null;
  scope_end: string | null;
  max_count: number | null;
  last_login_at: string | null;
}

export const auditorAuthService = {
  /**
   * POST /auditor/login — issues an auditor-scoped JWT.
   * Auditor accounts are keyed by email (not username) per the backend
   * AuditorService design (PRD-F1.1 §3.3).
   */
  async login(email: string, password: string): Promise<{ token: string; auditor: AuditorSession }> {
    return auditorApi.post('/auditor/login', { email, password });
  },

  /**
   * GET /auditor/me — current auditor session info.
   */
  async me(): Promise<AuditorSession> {
    return auditorApi.get('/auditor/me');
  },
};

// ===========================================================
// Admin-side auditor management
// ===========================================================

export interface CreateAuditorRequest {
  email: string;
  name: string;
  wallet_ids: number[];
  scope_start?: string;        // ISO 8601
  scope_end?: string;
  max_count?: number;          // max queries / disclosures auditor can issue
}

export interface CreateAuditorResponse {
  auditor_id: number;
  invitation_link: string;
  temp_password: string;       // shown ONCE on creation, give to auditor OOB
}

export const auditorAdminService = {
  /**
   * POST /auditors — create a new auditor (Admin only).
   */
  async create(req: CreateAuditorRequest): Promise<CreateAuditorResponse> {
    return api.post('/auditors', req);
  },

  /**
   * GET /auditors — list all auditors (Admin only).
   */
  async list(): Promise<AuditorSession[]> {
    return api.get('/auditors');
  },

  /**
   * POST /auditors/{id}/deactivate — Admin revokes auditor access.
   */
  async deactivate(id: number): Promise<{ ok: boolean }> {
    return api.post(`/auditors/${id}/deactivate`);
  },
};
