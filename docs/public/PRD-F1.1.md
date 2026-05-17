# PRD-F1.1 — Viewing Key Audit Layer + ZIP-307 Payment Disclosure

> Public release version. Internal team workflow + commit refs stripped.
> Original chapter structure preserved for traceability.

> **One-liner**: let the operator one-click export an Orchard viewing key to an
> external auditor + generate a ZIP-307-inspired payment disclosure report
> (PDF / CSV / JSON), delivering Zcash-native after-the-fact compliance
> auditability — more elegant than Privacy Pools ASP because the privacy
> stays on the protocol layer while the disclosure is selective and
> recipient-driven.

---

## §1 Context

- librustzcash 0.27 / Orchard 0.13 already integrated (see `backend/Cargo.toml`).
- `backend/src/blockchain/zcash/orchard/` ships Halo 2 proof builders + the
  4 Orchard transfer modes (T→T, T→Z, Z→Z, Z→T).
- `audit_logs` table already exists, **but only logs platform actions, not
  on-chain disclosure**.
- RBAC is currently 2-tier (`Admin` / `Operator` in `backend/src/db/models.rs`
  `UserRole`).
- **No viewing-key export API, no ZIP-307 disclosure generator, no Auditor
  role**. The existing `OrchardKeys` struct (`backend/src/blockchain/zcash/orchard/keys.rs`)
  carries viewing-key derivation capability internally but never reaches the
  service / handler layer.

---

## §2 Requirements

### 2.1 User Stories

1. **Operator (Admin)** — when auditors / regulators / tax authorities come
   knocking, **one-click export a viewing key** without ever exposing the
   spending key.
2. **Auditor** — log in to their own dedicated account, see assigned wallets'
   incoming + outgoing transfers + balance + disclosure history.
   **Cannot move funds, cannot edit configuration.**
3. **Regulator** — receives a ZIP-307 payment disclosure PDF / CSV that
   can be verified offline and reconstructs the fund flow within the
   selected window.

### 2.2 Functional Requirements (FR)

| ID | Requirement | Priority |
|---|---|---|
| F1.1.1 | Orchard **outgoing viewing key (OVK)** export API | P0 |
| F1.1.2 | Orchard **incoming viewing key (IVK)** export API | P0 |
| F1.1.3 | **Unified Full Viewing Key (UFVK)** export API (t-addr + Orchard) | P0 |
| F1.1.4 | **Auditor role** — third RBAC tier, isolated login from Admin/Operator | P0 |
| F1.1.5 | Admin invites Auditor + binds wallet scope (single / multi) + time window | P0 |
| F1.1.6 | Auditor **read-only dashboard**: scoped wallets' transfers / balance / past disclosures | P0 |
| F1.1.7 | **ZIP-307-inspired payment disclosure** generator (async) | P0 |
| F1.1.8 | Granularity control: single tx / single address / time range | P0 |
| F1.1.9 | Output formats: **PDF / CSV / JSON** | P0 |
| F1.1.10 | Every viewing-key export + disclosure generation lands in `audit_logs` | P0 |
| F1.1.11 | One-time viewing-key download (server zeroes plaintext after, keeps hash) | P0 |

### 2.3 Non-Functional Requirements (NFR)

| ID | Requirement |
|---|---|
| NFR-1 | Viewing keys **encrypted at rest** (reuse AES-256-GCM + per-tenant master key) |
| NFR-2 | Auditor data access **enforced at the DB layer** (middleware + repository double-check) |
| NFR-3 | Disclosure generation is **asynchronous** (large windows may take 30s+); UI polls |
| NFR-4 | Report files **TTL 7 days** (auto-cleanup after download or expiry) |
| NFR-5 | Reuse existing i18n framework — all UI strings ship Chinese + English |

### 2.4 Out of Scope (M1)

- ❌ Email / SMS auditor identity verification (M2 adds OTP)
- ❌ Multi-auditor co-signing on the same disclosure (M3)
- ❌ Raw Orchard note dump to auditor (aggregate report only)
- ❌ On-chain zk-disclosure proof (M5+ with ZIP-307 v2)

---

## §3 Technical Design

### 3.1 Viewing-key export

```
[Admin POST /viewing-keys/export]
        ↓
WalletService.export_viewing_key(wallet_id, key_type, password)
        ↓
OrchardKeys::derive_viewing_keys(spending_key)
        ↓
serialize → AES-GCM encrypt → DB `viewing_key_exports` row
        ↓
return one-time download token (TTL 24h, single-use)
```

- `key_type ∈ {ovk, ivk, ufvk}`
- Reuses the existing `crypto/` module's AES-256-GCM helpers
- Download URL uses a 32-byte random token (**not** a JWT) so it can be
  shared with the auditor independently of the admin's login session.

### 3.2 ZIP-307 Payment Disclosure generation

```
[Admin POST /wallets/{id}/payment-disclosures]
        ↓
PaymentDisclosureService.generate(wallet_id, granularity, scope_param, format)
        ↓
async task (tokio::spawn) → write DB status = "generating"
        ↓
[librustzcash + scanned orchard_notes] build disclosure body
        ↓
filter by granularity: tx_hash | recipient_addr | time_range (auto-mapped
to block height via ChainClient.block_at_timestamp for time-range queries)
        ↓
assemble disclosure JSON → render PDF / CSV
        ↓
persist to `payment_disclosures` row + file under `./uploads/disclosures/`
        ↓
flip status to "ready"; UI poll picks it up
```

- PDF: `printpdf` crate (pure-Rust, no system deps)
- CSV: `csv` crate, fixed 10-column header
- JSON: serde_json pretty-print

### 3.3 Auditor role implementation

- `users` table `role` column already VARCHAR — semantically add `Auditor` but
- Auditor data is **physically isolated** in its own `auditors` table
  (do not extend `users`; prevents the auditor from accidentally entering
  Admin/Operator surfaces)
- Auditor traffic flows through the `/api/v1/auditor/*` prefix
- Independent JWT secret (`WEB3_AUDITOR_JWT_SECRET`); admin and auditor
  sessions cannot cross-authenticate
- `AuditorAuthMiddleware` enforces scope on every request

### 3.4 Reuse inventory

| Existing component | How F1.1 reuses it |
|---|---|
| `crypto/` AES-256-GCM | Encrypts the exported viewing-key payload |
| `OrchardKeys` (`blockchain/zcash/orchard/keys.rs`) | Adds `derive_viewing_keys()` |
| `Wallet.encrypted_private_key` | Decrypted → derives viewing key → re-encrypts result |
| `audit_logs` table | Stores export + disclosure operation records |
| `AuthMiddleware` | Pattern source for `AuditorAuthMiddleware` |
| `WalletRepository` | New `find_by_id_with_auditor_scope(wallet_id, auditor_id)` method |

---

## §4 Data Model

> All tables are created at startup via Rust + sqlx `CREATE TABLE IF NOT EXISTS`
> (no separate .sql migration files).

### 4.1 `auditors`

```sql
CREATE TABLE IF NOT EXISTS auditors (
  id INT AUTO_INCREMENT PRIMARY KEY,
  email VARCHAR(255) NOT NULL UNIQUE,
  password_hash VARCHAR(255) NOT NULL,
  name VARCHAR(255) NOT NULL,
  invited_by_user_id INT NOT NULL,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  last_login_at DATETIME NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (invited_by_user_id) REFERENCES users(id),
  INDEX idx_email (email)
);
```

### 4.2 `auditor_wallet_scopes`

```sql
CREATE TABLE IF NOT EXISTS auditor_wallet_scopes (
  id INT AUTO_INCREMENT PRIMARY KEY,
  auditor_id INT NOT NULL,
  wallet_id INT NOT NULL,
  granted_by_user_id INT NOT NULL,
  scope_start_ts DATETIME NOT NULL,
  scope_end_ts DATETIME NOT NULL,
  max_disclosure_count INT NOT NULL DEFAULT 10,
  current_count INT NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (auditor_id) REFERENCES auditors(id),
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  UNIQUE KEY uniq_scope (auditor_id, wallet_id)
);
```

### 4.3 `viewing_key_exports`

```sql
CREATE TABLE IF NOT EXISTS viewing_key_exports (
  id INT AUTO_INCREMENT PRIMARY KEY,
  wallet_id INT NOT NULL,
  exported_by_user_id INT NOT NULL,
  key_type VARCHAR(16) NOT NULL,            -- ovk | ivk | ufvk
  encrypted_payload BLOB NOT NULL,          -- AES-GCM(viewing_key)
  payload_hash VARCHAR(64) NOT NULL,        -- SHA256(viewing_key) — audit proof
  download_token VARCHAR(64) NOT NULL UNIQUE,
  downloaded_at DATETIME NULL,
  downloaded_by_ip VARCHAR(64) NULL,
  expires_at DATETIME NOT NULL,             -- 24h from create
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (exported_by_user_id) REFERENCES users(id)
);
```

### 4.4 `payment_disclosures`

```sql
CREATE TABLE IF NOT EXISTS payment_disclosures (
  id INT AUTO_INCREMENT PRIMARY KEY,
  wallet_id INT NOT NULL,
  generated_by_user_id INT NOT NULL,
  granularity VARCHAR(16) NOT NULL,         -- tx | address | range
  scope_param JSON NOT NULL,                -- {tx_hash, address, from, to}
  tx_count INT NOT NULL DEFAULT 0,
  disclosure_json JSON NULL,                -- ZIP-307 payload (null while generating)
  format VARCHAR(16) NOT NULL,              -- pdf | csv | json
  file_path VARCHAR(512) NULL,
  status VARCHAR(16) NOT NULL DEFAULT 'generating',
  error_message TEXT NULL,
  expires_at DATETIME NOT NULL,             -- 7-day TTL
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (generated_by_user_id) REFERENCES users(id),
  INDEX idx_wallet_status (wallet_id, status)
);
```

---

## §5 API

### 5.1 Admin side (under `/api/v1/` with `AuthMiddleware`)

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/wallets/{id}/viewing-keys/export` | `{key_type, password}` | `{export_id, download_token, expires_at}` |
| GET | `/wallets/{id}/viewing-keys/exports` | — | `[{id, key_type, downloaded_at, expires_at}]` |
| GET | `/viewing-keys/download/{token}` | — | text/plain (ZIP-316 `uview…` for UFVK; hex for OVK/IVK) |
| POST | `/auditors` | `{email, name, wallet_ids[], scope_start, scope_end, max_count}` | `{auditor_id, invitation_link, temp_password}` |
| GET | `/auditors` | — | `[{id, email, name, scopes[], active}]` |
| PUT | `/auditors/{id}/deactivate` | — | `{ok}` |
| POST | `/wallets/{id}/payment-disclosures` | `{granularity, scope_param, format}` | `202 {disclosure_id, status:"generating", tx_count:0, ...}` |
| GET | `/payment-disclosures/{id}` | — | full row `{id, status, tx_count, disclosure_json?, error_message?, ...}` |
| GET | `/payment-disclosures/{id}/download` | — | binary file (PDF / CSV / JSON depending on format) |
| GET | `/wallets/{id}/payment-disclosures` | — | `[{id, status, granularity, created_at}]` |

### 5.2 Auditor side (`/api/v1/auditor/*` with `AuditorAuthMiddleware`)

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/auditor/login` | `{email, password}` | `{token, auditor: {...}}` |
| GET | `/auditor/me` | — | `{auditor, scopes[]}` |
| GET | `/auditor/wallets` | — | `[{wallet_id, wallet_name, address, chain, scope_start, scope_end, max_disclosure_count, current_count, total_tx_count, last_activity_at, pending_disclosures}]` (11-field summary per wallet) |
| GET | `/auditor/wallets/{id}/balance` | — | `{address, chain, native_balance, tokens[]}` (Zcash returns combined transparent + shielded) |
| GET | `/auditor/wallets/{id}/transfers` | `?limit=&offset=` | `{wallet_id, scope_start, scope_end, transfers[]}` (bounded to scope half-open window; backend clamps limit 1..200) |
| GET | `/auditor/wallets/{id}/disclosures` | — | scoped disclosure list |

### 5.3 Error codes

- `401 INVALID_AUDITOR_TOKEN` — admin token at `/auditor/*` (or vice versa)
- `403 OUT_OF_SCOPE` — auditor accesses an unscoped wallet or out-of-window transfer
- `429 DISCLOSURE_QUOTA_EXCEEDED` — exceeds `max_disclosure_count`
- `410 EXPORT_EXPIRED` — viewing-key download link expired or single-use already consumed

---

## §6 Frontend Spec (high level)

- **Admin side**: Wallet detail page gets a "Compliance" tab with "Export
  Viewing Key" (3-step flow: form → one-time token issued → token consumed
  on download) and "Generate Disclosure" actions. A separate "Auditor
  Management" page lists invited auditors + create/deactivate.
- **Auditor side**: Independent `/auditor/login`. After login, a Dashboard
  showing the 11-field per-wallet summary. Drill into a wallet for live
  balance + paged transfers. "New Disclosure" form picks granularity +
  scope_param + format; submission returns 202 with a `disclosure_id`,
  UI polls `GET /payment-disclosures/{id}` every 2s until `status === 'ready'`
  then auto-fetches the body.
- **i18n**: All viewing-key + disclosure copy ships zh + en. Keys live under
  `auditor.*` / `auditor.disclosure.*` / `auditor.viewing_key.*` namespaces.

---

## §7 Risk Register

| ID | Risk | Severity | Mitigation |
|---|---|---|---|
| R1 | librustzcash ZIP-307 API granularity insufficient (e.g. no time-range path) | 🔴 | Pre-implementation spike confirmed the scanned-notes path can synthesize the same body without depending on a particular librustzcash helper; the ChainClient-level block_at_timestamp lookup handles time-range mapping. |
| R2 | Viewing key leaked after one-time download | 🟠 | Server zeroes plaintext immediately on download; only payload hash remains; downloads logged with IP + timestamp; 24h TTL on the token. |
| R3 | Auditor SQL-layer escalation | 🟠 | Repository methods enforce `WHERE wallet_id IN (SELECT wallet_id FROM auditor_wallet_scopes WHERE auditor_id = ?)`; double-check both in middleware and at repo. |
| R4 | PDF rendering performance on 1000+ tx windows | 🟡 | Async generation + 200-row hard cap per PDF page; CSV/JSON unbounded; user picks format. |
| R5 | librustzcash version drift | 🟡 | Pinned 0.27 family; new deps require `cargo tree` audit. |

---

## Appendix · Glossary

- **OVK / IVK / UFVK** — Orchard outgoing / incoming / unified full viewing key.
- **ZIP-307** — Zcash improvement proposal for payment disclosure JSON.
- **Auditor** — Third RBAC role, independent of Admin / Operator.
- **Scope** — `(wallet × time-window × quota)` triple gating an auditor's access.
