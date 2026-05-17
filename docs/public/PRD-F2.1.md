# PRD-F2.1 — Maker / Checker Dual-Sign Transfers

> Public release version. Internal team workflow + commit refs stripped.
> Original chapter structure preserved for traceability.

> **One-liner**: split a transfer into **maker → checker → executor** three
> stages so corporate finance / CFO / compliance can — for the first time —
> trust real-world funds to a privacy-coin treasury platform, while
> delivering the "fund governance" pillar of the M1 roadmap.

---

## §1 Context

- Current RBAC is binary: `Admin` / `Operator` in `backend/src/db/models.rs`
  `UserRole`. No Auditor / Maker / Checker concept.
- `Transfer` state machine has 4 states (lines 87-95):
  ```
  Pending → Submitted → Confirmed | Failed
  ```
- The transfer flow is 2 segments: `POST /api/v1/transfers` (create Pending
  row) → `POST /api/v1/transfers/{id}/execute` (broadcast on-chain).
- **Any authenticated user (including Operator) can independently create +
  execute** with no approval gate.
- No notion of "fund limit," "multi-approver," "approval SLA," "timeout."
- `audit_logs` records actions but **cannot block** a single user from
  executing a large transfer.
- Same shape applies on the Orchard path (`POST /transfers/orchard` +
  `/execute`).

> This is the core of the M1 "compliance + fund-governance layer" — the
> precondition that lets enterprise finance / risk / audit departments
> first onboard to privacy-coin treasury.

---

## §2 Requirements

### 2.1 User stories

1. **Operator (Maker)**: I submit a transfer; the platform tells me
   "amount ≥ \$5k requires Admin approval"; transfer enters
   **AwaitingApproval** state; I see "awaiting approval" in My Transfers;
   the approver receives a notification and decides.
2. **Admin (Checker)**: my "Pending my approval" queue shows each item with
   amount / recipient / maker / memo / risk tag; I one-click Approve or
   Reject (with reason).
3. **Operator (Executor)**: after approval, I (or an automation) trigger
   execute. If not approved or expired, execute is rejected.
4. **Admin (Compliance)**: I configure approval policies — per-wallet
   threshold, per-token threshold, SLA, required approver count (M-of-N,
   v1 = 1-of-N).
5. **Auditor (read-only)**: I can see full approval history + timeline but
   cannot move funds or change policy.
6. **Any role**: a transfer exceeding SLA (default 24h) without approval
   auto-expires; the maker re-issues if needed; audit_logs preserves the
   full timeline.

### 2.2 Functional requirements (FR)

| ID | Requirement | Priority |
|---|---|---|
| FR-1 | Extend Transfer state machine — add `AwaitingApproval` / `Approved` / `Rejected` / `Expired` | P0 |
| FR-2 | New `transfer_approvals` table (approver / decision / reason / ts) | P0 |
| FR-3 | New `approval_policies` table — scope = global / wallet / user; match by `(chain, token, amount_threshold)` | P0 |
| FR-4 | `POST /transfers`: if `amount ≥ matched policy.threshold` → status = `AwaitingApproval` (not `Pending`); cannot directly execute | P0 |
| FR-5 | New `POST /transfers/{id}/approve` — Admin role, **maker ≠ checker** enforced | P0 |
| FR-6 | New `POST /transfers/{id}/reject` — `reason` required, ≥ 5 chars | P0 |
| FR-7 | Modify `POST /transfers/{id}/execute` — accept `Pending` (below threshold) OR `Approved` only | P0 |
| FR-8 | New `GET /approvals/pending` — list transfers awaiting my approval (filters out my own) | P0 |
| FR-9 | New `GET/POST/PUT/DELETE /approval-policies` — Admin manages policies | P0 |
| FR-10 | Background worker — scan `AwaitingApproval` past SLA → flip `Expired` + audit_log | P0 |
| FR-11 | `audit_logs` records 5 events: create_pending_approval / approve / reject / expire / execute_with_approval | P0 |
| FR-12 | Orchard path runs the same approval flow (`POST /transfers/orchard` / `/execute`) | P0 |
| FR-13 | Maker self-recall (`DELETE /transfers/{id}`) on `AwaitingApproval` only | P1 |
| FR-14 | Email notification stub (reuse the email channel from the notifications module): approval request / decision | P1 |
| FR-15 | Large-amount re-auth at approve time (≥ \$25k → re-verify password / TOTP) | P1 |

### 2.3 Non-functional requirements (NFR)

| ID | Requirement |
|---|---|
| NFR-1 | **Atomic state transitions** — approve + status flip in a single DB transaction; prevent two checkers approving concurrently |
| NFR-2 | **Backward compatibility** — existing clients calling `/transfers` + `/execute` for sub-threshold amounts continue to work unchanged |
| NFR-3 | **Audit completeness** — every transition writes `audit_logs.details` JSON with `from_status` / `to_status` / `actor_user_id` / `reason` |
| NFR-4 | **Idempotency** — approve / reject accept `Idempotency-Key` header; replay returns the original decision |
| NFR-5 | **Observability** — Prometheus metrics for pending_approval queue length / mean approval latency / SLA-timeout rate |
| NFR-6 | **MVP is 1-of-N** — N-of-M multi-stage approval defers to a later milestone; schema reserves `required_count` field (default 1) |
| NFR-7 | **maker ≠ checker as a hard constraint** — DB unique index (transfer_id, approver_user_id) + application-layer check `transfer.created_by ≠ approver.id` |
| NFR-8 | All new tables auto-created at startup via Rust + sqlx migrations (no separate .sql files) |

### 2.4 Out of scope

- ❌ N-of-M multi-stage approval (later milestone)
- ❌ On-chain multi-sig (different layer; revisit with HSM integration)
- ❌ Mobile push notifications
- ❌ Auto-approval rules (whitelisted recipients) — all human in MVP
- ❌ Integration with external approval systems (DocuSign / corporate workflow) — ERP-adapter scope

---

## §3 Technical Design

### 3.1 State machine

```
                          (low amount, no policy)
                          ┌─────────────────────┐
                          │                     ▼
   [maker creates] ──→ Pending ─── /execute ──→ Submitted ──→ Confirmed
                          │                                    │
                          │ (high amount,                      ▼
                          │  policy matched)                 Failed
                          ▼
                   AwaitingApproval ──── /approve ──→ Approved ──┐
                          │                                       │
                          │ /reject                               │ /execute
                          ▼                                       ▼
                       Rejected                                Submitted ──→ Confirmed | Failed
                          ▲
                          │ SLA timeout
                  (after expiry_at)
                          │
                       Expired

   DELETE /transfers/{id} is only allowed in AwaitingApproval (maker self-recall)
```

### 3.2 Design principles

1. **Approval and execution are separate states** — `Approved` is distinct so
   the checker and the executor can be different people; allows post-approval
   gas-window optimization.
2. **maker ≠ checker is a hard constraint** — the initiator cannot approve
   their own transfer (DB + application double-check).
3. **Policy match prefers most specific** — `user > wallet > global`; within
   the same tier, exact token match wins.
4. **SLA default 24h**, per-policy override; background worker scans expirations
   every 5 minutes.
5. **Existing API shapes preserved** — `POST /transfers` still returns
   `{id, status, ...}`; only the `status` enum gains new values. `/execute`
   semantics for low-amount calls are unchanged.
6. **RBAC reuse** — MVP does not add a new role; `Admin = Checker`,
   `Operator = Maker`. Admin can also create transfers (acting as Maker) but
   still cannot approve their own. Auditor read-only access wires up once
   F1.1 lands.

### 3.3 Policy match pseudocode

```rust
// backend/src/services/approval_policy_service.rs
fn requires_approval(transfer: &TransferRequest, user: &User) -> Option<&ApprovalPolicy> {
    // 1. user-scoped policy (most specific)
    if let Some(p) = repo.find_user_policy(user.id, &transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    // 2. wallet-scoped policy
    if let Some(p) = repo.find_wallet_policy(transfer.wallet_id, &transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    // 3. global policy (default fallback)
    if let Some(p) = repo.find_global_policy(&transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    None
}
```

### 3.4 Interaction with F1.1 / F3.1

- **F1.1 Auditor**: read-only access to the approval views once F1.1 ships.
- **F3.1 Payroll Run**: a run is a batch of transfers; **the run total**
  triggers approval at the run level (see PRD-F3.1) instead of per-item.
  After run-level approval, all items inside execute in a fan-out.

---

## §4 Data Model

> All tables are created at startup via Rust + sqlx migrations.

### 4.1 Transfer state enum

```rust
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum TransferStatus {
    Pending,           // existing — sub-threshold direct path
    AwaitingApproval,  // new
    Approved,          // new
    Rejected,          // new (terminal)
    Expired,           // new (terminal)
    Submitted,         // existing
    Confirmed,         // existing
    Failed,            // existing
}
```

### 4.2 `transfer_approvals`

```sql
CREATE TABLE IF NOT EXISTS transfer_approvals (
  id INT PRIMARY KEY AUTO_INCREMENT,
  transfer_id INT NOT NULL,
  approver_user_id INT NOT NULL,
  decision VARCHAR(16) NOT NULL,        -- 'approve' | 'reject'
  reason TEXT,                          -- reject required; approve optional
  policy_snapshot JSON,                 -- frozen policy at decision time
  idempotency_key VARCHAR(128),
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_approval_idem (idempotency_key),
  UNIQUE KEY uq_one_decision_per_approver (transfer_id, approver_user_id),  -- NFR-7
  INDEX idx_transfer (transfer_id),
  INDEX idx_approver (approver_user_id)
);
```

### 4.3 `approval_policies`

```sql
CREATE TABLE IF NOT EXISTS approval_policies (
  id INT PRIMARY KEY AUTO_INCREMENT,
  scope VARCHAR(16) NOT NULL,           -- 'global' | 'wallet' | 'user'
  scope_id INT,                         -- wallet.id or user.id; NULL for global
  chain VARCHAR(32) NOT NULL,
  token VARCHAR(32) NOT NULL,
  amount_threshold DECIMAL(36, 18) NOT NULL,
  sla_minutes INT NOT NULL DEFAULT 1440,
  required_count INT NOT NULL DEFAULT 1,
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  created_by INT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uq_scope_match (scope, scope_id, chain, token),
  INDEX idx_scope_lookup (scope, chain, token, enabled)
);
```

### 4.4 `transfers` column additions

```sql
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS expiry_at TIMESTAMP NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approval_required TINYINT(1) NOT NULL DEFAULT 0;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approved_at TIMESTAMP NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approved_by INT NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS rejection_reason TEXT NULL;
```

---

## §5 API

### 5.1 Modified: `POST /api/v1/transfers`

Request body unchanged. Response gains fields:

```json
{
  "id": 123,
  "status": "awaiting_approval",
  "approval_required": true,
  "expiry_at": "2026-05-17T16:00:00Z",
  "matched_policy_id": 7,
  ...
}
```

### 5.2 New: `POST /api/v1/transfers/{id}/approve`

```http
POST /api/v1/transfers/123/approve
Authorization: Bearer <admin-jwt>
Idempotency-Key: <client-uuid>

{ "note": "Q2 finance bonus, confirmed via CEO email" }
```

Response:

```json
{
  "transfer_id": 123, "approval_id": 456, "decision": "approve",
  "approver_user_id": 2, "approver_username": "alice",
  "approved_at": "2026-05-16T15:34:00Z", "transfer_status": "approved"
}
```

Error codes: `403 NotAuthorized` (non-Admin); `409 SelfApproveForbidden`
(caller is the maker — NFR-7); `410 TransferExpired`; `422 InvalidState`.

### 5.3 New: `POST /api/v1/transfers/{id}/reject`

Body `{ "reason": "Recipient not on KYB whitelist" }`. Same response /
error shape as `/approve`, plus `400 ReasonTooShort` if `reason < 5 chars`.

### 5.4 Modified: `POST /api/v1/transfers/{id}/execute`

- Status `Pending` (no policy match) → Submitted (unchanged behavior).
- Status `Approved` → checks approver / SLA / approver ≠ executor → Submitted.
- Status `AwaitingApproval` / `Rejected` / `Expired` → `422 InvalidState`.

### 5.5 New: `GET /api/v1/approvals/pending`

Returns transfers awaiting my approval (auto-filters out my own maker entries):

```http
GET /api/v1/approvals/pending?limit=20&cursor=eyJ0cyI6...
```

Response:

```json
{
  "items": [
    {
      "transfer_id": 123, "chain": "ethereum", "token": "USDT",
      "amount": "8500", "from_address": "0x..", "to_address": "0x..",
      "maker_username": "bob", "memo": "Vendor X invoice #2024-Q2-007",
      "matched_policy_id": 7, "expiry_at": "2026-05-17T16:00:00Z",
      "created_at": "2026-05-16T15:30:00Z"
    }
  ],
  "next_cursor": null
}
```

### 5.6 New: `/approval-policies` CRUD

Admin only. POST create / PUT update / GET list (with `scope` filter) /
DELETE. Update allows only `amount_threshold` / `sla_minutes` /
`required_count` / `enabled` — `scope` / `chain` / `token` are immutable
to keep audit snapshots stable.

### 5.7 `DELETE /api/v1/transfers/{id}` (FR-13)

Allowed only in `AwaitingApproval` and only when caller is the maker;
other states return `422`.

### 5.8 Orchard parallel surface

The Orchard transfer path (`POST /transfers/orchard` + `/execute`) runs
the same approval pipeline; `approval_service.rs` is chain-agnostic and
called by both flows.

---

## §6 Frontend Spec (high level)

1. **Transfer.tsx (modified)** — create form; on submit, if response carries
   `approval_required:true`, flash banner ("amount exceeds policy threshold")
   then redirect to "My pending approvals."
2. **MyPending.tsx (new)** — maker view of own awaiting/approved/rejected/
   expired transfers, with SLA countdown.
3. **Approval/Queue.tsx (new)** — checker queue, auto-filtered out own
   makers; row click to Detail.
4. **Approval/Detail.tsx (new)** — single-approval Approve / Reject
   (reason ≥ 5 chars) panel + approval timeline.
5. **Policies.tsx (modified, enumeration-first)** — chain via `/api/v1/chains`
   dropdown; token via hardcoded per-chain list (eth: ETH/USDT/USDC/DAI/WETH;
   zcash: ZEC); scope: global hides scope_id, wallet shows wallet dropdown
   filtered by chain, user shows numeric input.
6. **State-machine palette** — `awaiting_approval` yellow / `approved` green
   / `rejected` red / `expired` red with icon variant.
7. **Notifications** — in-app toast on decision + SLA-near-expiry warning (≤ 2h
   left turns red). Email and mobile push wired up via the notifications
   module in later milestones.
8. **i18n** — Chinese + English under `approval.*`, `approval.policies.*`,
   `approval.queue.*`, `approval.detail.*` namespaces.

---

## §7 Risk Register

| ID | Risk | Severity | Mitigation |
|---|---|---|---|
| RK-1 | Existing automation scripts break on `AwaitingApproval` | 🟠 | NFR-2 backward compat; default global threshold seeded high; new clients read `approval_required` |
| RK-2 | maker = checker hole (Admin approves own transfer) | 🔴 | DB unique index + application-layer double-check; transaction on `transfer_approvals` insert |
| RK-3 | SLA worker drops a row | 🟠 | Cron-style scheduler; Prometheus exporter alert; cold-boot one-time sweep on startup |
| RK-4 | Large transfer blocked by absent checker | 🟡 | SLA 24h default + email push + maker self-recall (FR-13) |
| RK-5 | Approval policy race (Admin edits policy while transfer pending) | 🟡 | Snapshot `matched_policy_id` + `policy_snapshot` JSON in `transfer_approvals` at decision time; policy edits are not retroactive |
| RK-6 | Orchard path missed → bypass | 🔴 | `approval_service.rs` shared; tests cover both chain handlers |
| RK-7 | Idempotency-key replay with different decision | 🟢 | DB uniq constraint returns original decision (treats as success) |
| RK-8 | F1.1 Auditor vs F2.1 view permissions | 🟢 | Auditor read-only on approval views; handler middleware splits |

---

## Appendix · Interface contract with F1.1 / F3.1

- F1.1 Auditor role: read-only on approval views (`GET` allowed; mutations
  rejected).
- F3.1 Payroll Run: the **run total** triggers approval at run level (see
  PRD-F3.1). After run-level approval, items execute in fan-out without a
  per-item approval gate.
- Shared `audit_logs.action` namespace:
  `transfer.create_pending_approval` / `transfer.approve` / `transfer.reject` /
  `transfer.expire` / `transfer.execute_with_approval` / `policy.create` /
  `policy.update`.
