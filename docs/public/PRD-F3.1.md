# PRD-F3.1 — Batch Payroll Run

> Public release version. Internal team workflow + commit refs stripped.
> Original chapter structure preserved for traceability.

> **One-liner**: turn the "payroll distribution" sketch from
> `docs/zcash-enterprise-use-cases-en.md` §8 into a **real backend
> entity** — import employees via CSV → distribute funds to N employees
> from a single source wallet → retry on partial failure → status fully
> queryable. **Single-chain per run**, no cross-chain swap.

---

## §1 Context

- `docs/zcash-enterprise-use-cases-en.md` §8 sketches a payroll flow as a
  shell script — not a backend entity.
- The existing `transfers` table is **single-leg** (one record = one on-chain
  transfer). No notion of a batch.
- `POST /api/v1/transfers/orchard` is currently **single-payee**; no
  multi-output fan-out path exists at the service layer.
- librustzcash / Orchard 0.13 internally supports multi-output bundles
  (`OrchardTransactionBuilder::add_recipient`), but the service layer never
  exposes it.
- No employee directory; no CSV import; no per-batch retry mechanism.

---

## §2 Requirements

### 2.1 User stories

> **As** a corporate HR / Finance lead
> **I want to** upload a CSV at month-end (employee + address + amount + memo)
> and execute payroll with a single button
> **so that** I don't have to manually issue 50 separate transfers, and
> employees can't infer each other's salaries from on-chain data.

### 2.2 Functional requirements (FR)

| ID | Requirement | Priority |
|---|---|---|
| F3.1.1 | New `employees` table: `employee_code` / `name` / `wallet_address` / `chain` / `tags JSON` / `active`. **The tags JSON column absorbs M2 fields** (preferred_token / privacy_mode / kyc_status) without schema migration | P0 |
| F3.1.2 | New `payroll_runs` table — batch entity (pay_period / source_wallet_id / total_amount / status / etc.) | P0 |
| F3.1.3 | New `payroll_items` table — per-item entry (run_id / employee_id / address / amount / memo / status / tx_hash) | P0 |
| F3.1.4 | API to create a payroll run + upload items (CSV or JSON body) | P0 |
| F3.1.5 | CSV pre-validation — address legality / amount > 0 / employee existence / total ≤ wallet balance | P0 |
| F3.1.6 | API to execute a run — per-item fan-out on the wallet's chain | P0 |
| F3.1.7 | Surface per-item status — tx_hash / on-chain confirmation / failure reason | P0 |
| F3.1.8 | Partial-failure handling — one failed item does not block the others; run status becomes `partial_success` | P0 |
| F3.1.9 | Per-item retry — re-issue a single failed item without re-running the whole batch | P0 |
| F3.1.10 | Run report export (CSV) with employee_id / amount / tx_hash / status / timestamps | P0 |
| F3.1.11 | Entire run + per-item important actions (retry / cancel) log into `audit_logs` | P0 |
| F3.1.12 | maker / checker integration — large-total runs auto-pivot to `awaiting_approval` (uses F2.1 hook) | P0 |

### 2.3 Non-functional requirements (NFR)

| ID | Requirement |
|---|---|
| NFR-1 | Single run cap: **100 employees**; larger batches split into multiple runs (M1 limit; later milestone raises to 500) |
| NFR-2 | Per-item flow reuses the single-transfer path so retry semantics + audit trail match the existing `transfers` table behavior |
| NFR-3 | **Privacy**: ZCash payroll fans out into Orchard shielded notes so on-chain observers can't reconstruct the recipient list |
| NFR-4 | **Atomicity** — partial success is a valid run terminal state (`partial_success`); already-confirmed items are never re-sent on retry |
| NFR-5 | **Audit completeness** — every state change (create / execute / item submit / item confirm / item fail / retry / cancel) writes `audit_logs` |
| NFR-6 | **Backward compatibility** — existing `/transfers/orchard` API is unchanged; new functionality lives under the `/payroll/*` prefix |

### 2.4 Out of scope (M1)

- ❌ **Cross-token swap** (USDT / USDC / ETH → ZEC) — out of scope; revisit
  when on-chain swap rails mature.
- ❌ Streaming payroll (Sablier-style) — one-shot batch only.
- ❌ Tax calculations / legally-formatted payslip PDFs — external system concern.
- ❌ Employee self-service portal (employees viewing their payslip) — later milestone.
- ❌ Single ETH transaction with multiple outputs — ETH has no native
  multi-output semantics so we keep per-item fan-out for ETH payrolls.

---

## §3 Technical Design

### 3.1 Main flow

```
[Admin POST /payroll/runs (CSV)]
        ↓
PayrollService.create_run(source_wallet_id, csv_file)
        ↓
csv parser → validate address + amount + employee existence + total ≤ balance
        ↓
write payroll_runs (status=pending) + payroll_items (status=pending)
        ↓
respond with run_id + item_count + validation_errors[]

[Admin POST /payroll/runs/{id}/execute]
        ↓
PayrollService.execute_run(run_id)
        ↓
match approval_policy: run.total_amount vs policy.threshold
        ↓
   ┌─────────────────────────────────────────────────────────────┐
   │ Path A: run.total ≥ matched policy.threshold                 │
   │   run.status → awaiting_approval                             │
   │   return ExecuteRunOutcome::AwaitingApproval                 │
   │   wait for F2.1 approval → status flips to approved → caller │
   │   re-invokes /execute to take Path B                         │
   └─────────────────────────────────────────────────────────────┘
   ┌─────────────────────────────────────────────────────────────┐
   │ Path B: no policy match OR run already approved              │
   │   foreach item:                                              │
   │     chain_client.transfer_native(item.address, item.amount)  │
   │     write transfers row + payroll_items.transfer_id          │
   │     mark item submitted / failed                             │
   │   final run status: completed / partial_success / failed     │
   │   return ExecuteRunOutcome::Executed                         │
   └─────────────────────────────────────────────────────────────┘
        ↓
poll on-chain confirmation → items: submitted → confirmed (block_number set)
```

### 3.2 Reuse inventory

| Existing component | How F3.1 reuses it |
|---|---|
| `chain_client.transfer_native` (single-payee path) | Per-item fan-out — every PayrollItem becomes one independent tx, preserves the mature single-transfer broadcast + signing logic |
| `transfer_service` execute path | Each payroll item writes a real `transfers` row; the existing transfer history surface continues to display these without change |
| `WalletService.get_active_wallet()` | Source wallet lookup |
| `audit_logs` | Per-state-transition log writes |
| `approval_policy_repo` (F2.1) | Run-level threshold matching for the awaiting-approval pivot |

### 3.3 CSV format

```csv
employee_code,employee_address,amount,memo
EMP-001,u1qwerty...alice,3.85,2026-05 salary
EMP-002,u1qwerty...bob,4.62,2026-05 salary
EMP-003,u1qwerty...charlie,1.15,2026-05 bonus
```

- Header row required.
- Memo ≤ 512 bytes per row.
- Total rows ≤ NFR-1 (M1: 100).

### 3.4 ExecuteRunOutcome tagged union

The execute endpoint returns one of two shapes (`#[serde(tag="result",
rename_all="snake_case")]`):

```jsonc
// Path A — auto-pivot to approval
{
  "result": "awaiting_approval",
  "run_id": 7,
  "policy_id": 3,
  "threshold": "100.000000000000000000"
}

// Path B — actually executed (may be partial)
{
  "result": "executed",
  "run_id": 7,
  "submitted": 50,
  "failed": 2,
  "final_status": "partial_success"
}
```

The frontend `RunDetail.tsx` page branches on the `result` tag:
`awaiting_approval` redirects to the F2.1 approval queue; `executed`
renders the submitted / failed counts and item-level tx hashes.

---

## §4 Data Model

> Tables auto-created at startup via Rust + sqlx migrations.

### 4.1 `employees`

```sql
CREATE TABLE IF NOT EXISTS employees (
  id INT AUTO_INCREMENT PRIMARY KEY,
  employee_code VARCHAR(64) NOT NULL UNIQUE,
  name VARCHAR(255) NOT NULL,
  wallet_address VARCHAR(255) NOT NULL,
  chain VARCHAR(32) NOT NULL DEFAULT 'zcash',
  tags JSON NULL,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  INDEX idx_active (active),
  INDEX idx_address (wallet_address)
);
```

### 4.2 `payroll_runs`

```sql
CREATE TABLE IF NOT EXISTS payroll_runs (
  id INT AUTO_INCREMENT PRIMARY KEY,
  pay_period VARCHAR(32) NOT NULL,
  source_wallet_id INT NOT NULL,
  total_amount DECIMAL(28, 8) NOT NULL,
  item_count INT NOT NULL,
  status VARCHAR(24) NOT NULL DEFAULT 'pending',
    -- pending | awaiting_approval | approved | rejected
    --   | executing | completed | partial_success | failed | cancelled
  created_by_user_id INT NOT NULL,
  executed_by_user_id INT NULL,
  executed_at DATETIME NULL,
  notes TEXT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  FOREIGN KEY (source_wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (created_by_user_id) REFERENCES users(id),
  INDEX idx_status (status),
  INDEX idx_period (pay_period)
);
```

### 4.3 `payroll_items`

```sql
CREATE TABLE IF NOT EXISTS payroll_items (
  id INT AUTO_INCREMENT PRIMARY KEY,
  run_id INT NOT NULL,
  employee_id INT NULL,                    -- null = ad-hoc, not bound to roster
  employee_address VARCHAR(255) NOT NULL,
  amount DECIMAL(28, 8) NOT NULL,
  memo TEXT NULL,
  status VARCHAR(24) NOT NULL DEFAULT 'pending',
    -- pending | building | submitted | confirmed | failed | compensation_pending
  tx_hash VARCHAR(128) NULL,
  block_number BIGINT NULL,
  transfer_id INT NULL,
  error_message TEXT NULL,
  retry_count INT NOT NULL DEFAULT 0,
  last_attempt_at DATETIME NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  FOREIGN KEY (run_id) REFERENCES payroll_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (employee_id) REFERENCES employees(id),
  FOREIGN KEY (transfer_id) REFERENCES transfers(id),
  INDEX idx_run_status (run_id, status),
  INDEX idx_tx_hash (tx_hash)
);
```

> The `transfers` table also gains a nullable `payroll_item_id` column so
> existing transfer-history surfaces continue to work without code change.

---

## §5 API

| Method | Path | Body | Response |
|---|---|---|---|
| POST | `/payroll/employees` | `{employee_code, name, wallet_address, chain, tags}` | `{employee_id}` |
| GET | `/payroll/employees` | `?active=&q=` | `[employee...]` |
| PUT | `/payroll/employees/{id}` | partial | full row |
| DELETE | `/payroll/employees/{id}` | — | `{ok}` (soft-delete: `active=false`) |
| POST | `/payroll/runs` | `{pay_period, source_wallet_id, notes, items[]}` | 201 `{run_id, item_count, validation_errors:[]}` OR 422 `{run_id:0, item_count:0, validation_errors:[{row_index, field, message}]}` |
| GET | `/payroll/runs` | `?limit=&offset=` | `[PayrollRun...]` |
| GET | `/payroll/runs/{id}` | — | `{run: {...}, items: [...]}` (nested) |
| POST | `/payroll/runs/{id}/execute` | — | `ExecuteRunOutcome` tagged union (see §3.4) |
| POST | `/payroll/runs/{id}/cancel` | — | `{ok}` (allowed from `pending` / `awaiting_approval` / `executing` for stuck recovery) |
| POST | `/payroll/runs/{run_id}/items/{item_id}/retry` | — | `{retried: true}` (only on `failed` items) |
| GET | `/payroll/runs/{id}/report` | `?format=csv|pdf` | binary file |

### 5.1 Error codes

- `400 INVALID_CSV` — header missing / column count mismatch
- `400 INVALID_ADDRESS` — one or more rows carry an invalid recipient
- `400 INSUFFICIENT_BALANCE` — wallet balance < `total_amount` (checked at execute time, not at create)
- `409 RUN_NOT_PENDING` — execute called on a run that's already executing / completed / failed
- `422 VALIDATION_ERRORS` — create_run returned non-empty validation_errors[]
- `429 RUN_ITEM_LIMIT` — items count exceeds NFR-1

---

## §6 Frontend Spec (high level)

- **Employees.tsx** — directory CRUD; supports single-add and (later)
  bulk CSV employee import.
- **RunCreate.tsx** — wizard: pick source wallet → fill pay_period → upload
  CSV → client-side preview parses + flags bad rows red → "Create payroll
  run" submits valid rows; server-side 422 errors stream back as inline
  per-row banners.
- **RunList.tsx** — table of runs with status pill, source wallet name (via
  `walletService.listWallets()` lookup), item count, created_at.
- **RunDetail.tsx** — `{run, items}` nested view; execute button branches on
  `ExecuteRunOutcome.result`: `awaiting_approval` flashes outcome card +
  auto-redirects to `/approval/pending`; `executed` shows submitted /
  failed / final_status. Retry button fans out per-failed-item client-side.
  Cancel button supports `pending` / `awaiting_approval` / `executing`
  (stuck recovery).
- **i18n** — Chinese + English under `payroll.run_status.*`,
  `payroll.create.*`, `payroll.detail.*` namespaces.

---

## §7 Risk Register

| ID | Risk | Severity | Mitigation |
|---|---|---|---|
| R1 | librustzcash multi-output Orchard tx bundle size / proof time | 🔴 | Per-item fan-out (one tx per recipient) keeps proof time bounded; single-tx multi-output Orchard is a later milestone optimization for fee + on-chain compactness |
| R2 | Partial-tx atomicity across many items | 🟠 | NFR-4: per-item commits are independent; `partial_success` is a first-class terminal status; failed items can be retried without re-sending confirmed ones |
| R3 | Employee address typo → burned funds (irreversible) | 🔴 | Strict pre-validation (`chain_client.validate_address`); employee directory + roster-by-code reduces typo surface vs ad-hoc CSV; later milestone adds historical-recipient confirmation |
| R4 | Large payroll triggers compliance review | 🟠 | NFR-1 + F2.1 maker/checker hook on run total: large totals auto-pivot to `awaiting_approval` |
| R5 | 100-employee stress: Orchard proof generation time | 🟠 | Per-item fan-out (single-output proofs) keeps each item's proof time bounded; async execute + UI polling required |
| R6 | Mainnet RPC node availability | 🟡 | Shared with the F1.1 RPC concern: use a local zebra node where available, fall back to public RPC |
| R7 | CSV encoding / BOM / multi-byte memo issues | 🟡 | csv crate UTF-8 enforced + automatic BOM stripping + documentation note |

---

## Appendix · Glossary

- **payroll_run** — One batch-payroll entity (one CSV upload = one run).
- **payroll_item** — Per-employee line inside a run.
- **fan-out** — Distribute a batch's funds across N recipients (M1 = per-item
  independent transactions; later milestone = single multi-output Orchard tx).
- **ExecuteRunOutcome** — Tagged union returned by `POST /payroll/runs/{id}/execute`,
  carrying either an awaiting-approval pivot or an executed summary.
