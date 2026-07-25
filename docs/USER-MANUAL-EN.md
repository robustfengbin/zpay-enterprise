# zPay Enterprise — User Operations Manual

> End-to-end operations guide for finance, compliance, and audit roles.
> Staging companion: <https://staging.example.com>
> For technical detail see the [PRD docs](.) and [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md).
> 中文版本: [USER-MANUAL-CN.md](USER-MANUAL-CN.md)

---

## Contents

- [1. Document map and role definitions](#1-document-map-and-role-definitions)
- [2. First-time setup after deployment](#2-first-time-setup-after-deployment)
- [3. Audit and compliance](#3-audit-and-compliance)
- [4. Employees and bulk payroll](#4-employees-and-bulk-payroll)
- [5. End-to-end business scenarios](#5-end-to-end-business-scenarios)
- [6. Troubleshooting and FAQ](#6-troubleshooting-and-faq)
- [7. Security guidance and pitch points](#7-security-guidance-and-pitch-points)
- [8. Ironwood migration and batch privacy transfers (F4)](#8-ironwood-migration-and-batch-privacy-transfers-f4)

---

## 1. Document map and role definitions

### 1.1 What does this system do

zPay Enterprise is an integrated **enterprise Web3 wallet + payroll + audit** platform. It does three things:

1. **Bulk payroll** — CFO/HR sends N employees their salary or project bonuses every month. Manual click-N-times becomes one CSV upload + one button click.
2. **Maker-checker risk control** — Large transfers automatically enter the approval queue. **Maker ≠ checker** (the initiator cannot approve their own transfer), preventing single-point insider risk.
3. **Privacy-preserving audit** — External auditors see the real on-chain history of authorized wallets, plus one-time disclosure reports, all **without holding any private key**. Compliant with regulatory requirements.

Chains supported: Ethereum (ETH / ERC20) and Zcash (transparent + shielded).

### 1.2 The three user roles

```
┌─────────────────────────────────────────────────────────────────┐
│  Admin                                                           │
│  ├─ Creates wallets, configures approval policies, invites       │
│  │  auditors                                                     │
│  ├─ Inherits everything an Operator can do                       │
│  └─ Cannot see the Auditor view (dual-JWT physical separation)   │
├─────────────────────────────────────────────────────────────────┤
│  Operator (M1 default = same role as Admin)                      │
│  ├─ Initiates transfers, initiates batch payrolls                │
│  ├─ Can approve transfers initiated by others (NOT their own)    │
│  └─ Cannot change approval policies, cannot create wallets       │
├─────────────────────────────────────────────────────────────────┤
│  Auditor                                                         │
│  ├─ Separate login at /auditor/login                             │
│  ├─ Sees only wallets the Admin has explicitly granted, and only │
│  │  inside the granted time window                               │
│  ├─ Requests disclosure reports (PDF / CSV / JSON)               │
│  └─ Strictly read-only — cannot transfer, cannot change config   │
└─────────────────────────────────────────────────────────────────┘
```

**Key design**: Admin and Auditor JWTs are **physically isolated** (different `kind` field). They cannot cross-access. Auditors must sign in at `/auditor/login`; an Admin who wants to use the Auditor view has to sign out first and sign in as an Auditor.

---

## 2. First-time setup after deployment

### 2.1 First login

Once deployment is done, visit <https://staging.example.com> (or your domain) and use the login page:

- Username: `admin`
- Password: auto-generated on first backend boot and written to `backend/.env.secrets` (chmod 0600, ops-only)
- Ops hands you the initial password through a secure channel (1Password / Vault / DM) — never group chat.

⚠️ **Required: change the password immediately after first login.**
- Top-right `admin` → Change Password → new password ≥ 12 chars, mix of letters and digits.
- Once changed, the `.env.secrets` value is dead (it was just a bootstrap).

### 2.2 Create wallets

Sidebar **Blockchain** → **Wallets** → **New Wallet**.

| Field | Notes |
|---|---|
| Name | Internal label, e.g. `Main-Treasury-ZEC` / `Project-A-USDT` |
| Chain | `ethereum` or `zcash` |

The backend generates the private key and encrypts it at rest with AES-GCM (it never sits as plaintext on disk). **Important**:
- The wallet's private key can only be exported **once** via Export Private Key (requires a second admin password entry as a fresh-intent check).
- Best practice: export, copy into a hardware wallet or offline USB, **never store it in cloud drives**.

### 2.3 Activate a wallet

Each chain has exactly one **active wallet** — outgoing transfers default to it.

- Wallets list → pick wallet → **Set Active**.
- Wallet detail shows: balance (transparent + shielded for Zcash), transfer history, Orchard sync progress.

### 2.4 (Optional) Seed the wallet with a first deposit

In staging: send a small amount (ZEC 0.001 / ETH 0.001) from another wallet to the staging wallet, wait for chain confirmation, balance updates.

In production: top up from your existing corporate wallet / exchange withdrawal into the active wallet's address.

---

## 3. Audit and compliance

### 3.1 Configure an approval policy (which transfers require dual-signature)

Sidebar **Governance & Compliance** → **Approval Policies** → **New Policy**

```
┌────────────────────────────────────────────────────────┐
│  scope        : global | wallet | user                  │
│  ├─ global    = applies to every wallet                 │
│  ├─ wallet    = applies only when this specific wallet  │
│  │              is the source                           │
│  └─ user      = applies only when this specific user    │
│                 initiates                               │
├────────────────────────────────────────────────────────┤
│  chain + token : e.g. ethereum + USDT                  │
├────────────────────────────────────────────────────────┤
│  amount threshold : e.g. 1000 (USDT)                   │
├────────────────────────────────────────────────────────┤
│  SLA minutes  : e.g. 120 — auto-expired if no one      │
│                 decides in this window                  │
│  required N   : M1 = 1 ; M2 supports N-of-M             │
└────────────────────────────────────────────────────────┘
```

**Matching priority** (a transfer may hit multiple policies):
1. **user** scope beats wallet scope
2. **wallet** scope beats global scope
3. Within the same scope: pick the policy with the **smallest threshold** that the amount actually meets.

**Common configurations**:

| Business need | Policy |
|---|---|
| All large USDT transfers dual-signed | `global / ethereum / USDT / 1000 / SLA 120 / 1 approver` |
| Only finance wallet's large ZEC dual-signed | `wallet=5 / zcash / ZEC / 10 / SLA 240 / 1 approver` |
| New hire is high-risk, audit everything | `user=42 / ethereum / USDT / 0.01 / SLA 60 / 1 approver` |
| No approvals at all | Don't configure any policy = every transfer executes directly |

### 3.2 Approval flow (dual-signature scenario)

```
 Maker                              System                  Checker
─────────────                    ──────────              ───────────────
1) Initiate 100 USDT ──────────► match policy / threshold 800
                                  │
                                  ├─ no match → execute directly
                                  │
                                  └─ matched → status                notification (M2)
                                     awaiting_approval ────────────► queue shows this row
                                     records expiry_at
                                                                       │
                                                                       ├ Approve (optional note)
                                                                       │   → status: approved
                                                                       │
                                                                       └ Reject (mandatory ≥ 5
                                                                         char reason)
                                                                          → status: rejected
                                                                
                                  ┌────────────────────────────────────────┐
                                  │  SLA Worker (sweeps every 5 minutes)    │
                                  │  flips overdue rows to "expired"        │
                                  └────────────────────────────────────────┘

3) After approval, maker clicks Execute → real on-chain send → status: confirmed
```

**Hard rule (NFR-7)**:
- **maker ≠ checker** — enforced at the SQL layer (`WHERE initiated_by <> viewer_user_id`). Even if the frontend RBAC has a bug, the DB still blocks the maker from approving their own transfer.
- Therefore **a single-user environment cannot demonstrate dual-signature** — you must create a second user account to act as approver.

### 3.3 Invite an auditor

Sidebar **Governance & Compliance** → **Auditor Management** → **Invite Auditor**

| Field | Notes |
|---|---|
| Email | Auditor's email (used only as username; the system does not send mail) |
| Name | Display only |
| Visible wallets | Multi-select the wallets you authorize them to view |
| Window start / end | They can only query chain data inside this window (e.g. 2026-Q1) |
| Disclosure budget | e.g. 10 / quarter — prevents abuse |

**On submit, the system returns**:
- `temp_password` (24-hour TTL, becomes invalid after first successful login)
- An "Open auditor login in a new tab" button.

**Security rules**:
- `temp_password` is **shown only once** at creation time. After the modal closes the system retains only a hash.
- Hand it off via **DM / 1Password / Vault** — **never in group chat, email, or screenshots**.
- The auditor should change the password immediately on first login.

### 3.4 Export viewing keys (let auditors verify ZEC history independently)

Looking at the zPay UI alone is not always enough — the auditor may want to use their own Zashi wallet to verify independently. That needs a viewing key.

Sidebar **Wallets** → pick a zcash wallet → **Export Viewing Key**

| Option | Who uses it | Notes |
|---|---|---|
| **OVK** (Outgoing Viewing Key) | Only wants to see transactions **sent** by this wallet | Minimum information |
| **IVK** (Incoming Viewing Key) | Only wants to see transactions **received** by this wallet | Medium |
| **UFVK** (Unified Full Viewing Key) | Full audit — incoming + outgoing | Maximum; emitted as ZIP-316 standard `uview...` string ready to paste into Zashi |

**Procedure**:
1. Click Export → re-enter admin password (fresh-intent check guarding against a leaked JWT).
2. The system returns a 24-hour download URL (base64url token).
3. DM the URL to the auditor.
4. The auditor opens it **exactly once** — the token is consumed. A second access returns 410 Gone.
5. The auditor pastes the `uview...` string into Zashi to see full history.

**Audit log**: every export writes a row to `viewing_key_exports` (who, when, IP, key hash). Even after the token is consumed the audit trail remains complete.

### 3.5 Auditor's own operations

Once the auditor has the email + temp_password:

1. Visit <https://staging.example.com/auditor/login> (**note the URL is different** — separate entry).
2. After login, the **Auditor Dashboard** shows 11 fields per authorized wallet:
   - Wallet name / address / chain
   - Scope window
   - Disclosure budget `current/max`
   - Total tx count / last activity / pending disclosures
3. Click a wallet → **WalletDetail** → see:
   - Real on-chain balance (transparent + shielded both shown)
   - Real transfer history inside the scope window (paginated)
4. Click **New Disclosure** → choose granularity and format:

```
granularity:
├─ tx        → single transaction detail (input tx_hash)
├─ address   → entire history of this wallet (all incoming notes in scope)
└─ range     → date range (auto-resolved to block height)

format:
├─ json   → full structured data (engineer-friendly)
├─ csv    → 10-column fixed schema (Excel-friendly)
└─ pdf    → A4 single page + table (regulator / print archive)
```

5. After submit the status is `generating`. The frontend polls every 2 seconds and usually flips to `ready` in under one second.
6. Click **Download** → the browser saves a PDF / CSV / JSON.

**Disclosure body** (ZIP-307 inspired enterprise format):
- Per transaction: tx_hash / block height / amount (ZEC + zatoshis both) / memo / **revealed nullifier** (lets the auditor independently verify the transaction exists on chain) / spent status.
- Header: wallet address / granularity / resolved range (block height and timestamp both).

---

## 4. Employees and bulk payroll

> This chapter is aimed at CFO / HR / Finance executors. Bulk payroll is the
> highest-frequency daily operation; the goal is to collapse N salary
> transfers per month into "one upload + one confirm."

### 4.1 The employee roster (configure once, reuse forever)

Sidebar **Payroll** → **Employees**

You must enroll employees in the system before you can pay them. The system
uniquely identifies an employee by `employee_code` + wallet address. Once
enrolled, payroll CSVs reference only `employee_code` — you never need to
rewrite the wallet address.

| Field | Notes | Example |
|---|---|---|
| Employee code (`employee_code`) | Your internal payroll ID, globally unique | `E001` / `ENG-042` |
| Name | Display only | Wang Xiaoming |
| Wallet address | Provided by the employee | `u1...` (Zcash) or `0x...` (ETH) |
| Chain | Employee's default receiving chain | zcash (recommended) or ethereum |
| Tags (JSON) | Free-form: department / level / KYC status / preferred token, etc. | `{"dept":"Engineering","kyc":"verified"}` |
| Active | False removes the employee from payroll selectors but keeps history | true |

**How to enroll**:
1. **Single new** — click New, fill fields. Good for onboarding one new hire.
2. **CSV bulk import** — prepare `code,name,wallet_address,chain,tags` CSV
   and import all at once (M1: paste into the new-employee form; a dedicated
   import endpoint lands later).
3. **Edit / soft delete** — edits take effect immediately. Delete is soft
   (data retained + active flipped to false); related payroll history stays
   linked, **records are never hard-deleted**.

> 💡 **Design intent**: employees are not one-off — soft-delete preserves
> historical payroll runs; KYC / department lives in `tags JSON` so adding a
> new field doesn't require a schema migration.

### 4.2 Payroll-specific approval policy (trigger dual-signature on monthly total)

Payroll is a large-amount operation. Best practice is to attach a dedicated
approval policy to the wallet you pay from (see §3.1):

```
┌───────────────────────────────────────────────────────────────┐
│  Typical enterprise configuration                              │
├───────────────────────────────────────────────────────────────┤
│  scope:        wallet=<corporate ZEC payroll wallet ID>        │
│  chain+token:  zcash + ZEC                                     │
│  amount:       1000 ZEC   ← run total ≥ 1000 → approval        │
│  SLA:          240 min    ← decide within 4h or auto-expire    │
│  required:     1 (M1)                                          │
└───────────────────────────────────────────────────────────────┘
```

**Key detail**: payroll triggers approval on the **batch total**, not on
each employee's amount. Example: 50 employees × 0.5 ZEC each = 25 ZEC total
→ 25 ZEC vs policy threshold 20 → the entire run goes through one approval,
not 50 separate approvals (this avoids drowning the approver in 50 notifications).

### 4.3 Create a payroll run (New Run)

Sidebar **Payroll** → **Payroll Runs** → **New Run**

| Field | Notes |
|---|---|
| Source wallet | The paying wallet; the dropdown shows only wallets you've created; chain is implied by the wallet (zcash wallet = entire batch in ZEC; ethereum wallet = USDT/USDC/ETH) |
| Pay period | Business label, e.g. `2026-05`; used as a query index, doesn't affect execution |
| Notes | Optional internal note |
| CSV file | Employee items, 4 columns: `employee_code, employee_address, amount, memo` |

**CSV example** (4 employees, 0.5 ZEC each plus one bonus, memo visible to recipient):

```csv
employee_code,employee_address,amount,memo
E001,u1abc...xyz,0.5,Salary 2026-05
E002,u1def...uvw,0.5,Salary 2026-05
E003,u1ghi...rst,0.5,Salary 2026-05 + bonus
E004,u1jkl...opq,0.7,Salary 2026-05 + bonus
```

**Two-stage validation**:
1. **Client-side** (runs in the browser immediately after upload):
   - Displays an N-row preview table.
   - Highlights bad rows in red (missing address / amount ≤ 0 / malformed).
   - Counter: ✅ valid X / ❌ invalid Y / total.
   - The Create button only submits valid rows.
2. **Server-side** (runs on confirm click):
   - The backend re-validates each row against the wallet's chain: is the
     address chain-valid? amount > 0? does the `employee_code` exist in the
     roster?
   - Any invalid row → **whole batch rejected** (HTTP 422), response carries
     `validation_errors: [{row_index, field, message}]` which the frontend
     renders inline.
   - All valid → run created in `pending` status.

> ⚠️ **Why both stages?** Client-side catches the obvious errors quickly
> (saves a round trip). Server-side is the source of truth (defends against
> CSV tampering and frontend bypass).

### 4.4 Execute the run — two outcome paths

Open **Payroll Runs** → your just-created run → **Execute**.

The backend first checks for matching approval policies (§4.2) using the
**run total** vs the threshold:

```
                         ┌─── Path A: triggers approval ─────────────┐
                         │                                          │
Execute clicked          │    total ≥ any enabled policy threshold   │
   │                     │    ↓                                     │
   ├──→ payroll_service ─┤    run.status → awaiting_approval        │
   │      .execute_run() │    NOT on-chain ❄️                        │
   │                     │    returns {result:"awaiting_approval",  │
   │                     │             policy_id, threshold}        │
   │                     │    frontend flashes 1.2s then auto-      │
   │                     │    redirects to /approval/pending        │
   │                     │                                          │
   │                     └──────────────────────────────────────────┘
   │
   │                     ┌─── Path B: direct execution ──────────────┐
   │                     │                                          │
   │                     │    no match OR total < threshold          │
   │                     │    ↓                                     │
   └─────────────────────┤    loop items → chain_client.transfer    │
                         │       per-item fan-out on-chain          │
                         │    returns {result:"executed",           │
                         │             submitted: N, failed: M,     │
                         │             final_status:                │
                         │             completed|partial_success|failed}│
                         │                                          │
                         └──────────────────────────────────────────┘
```

**Result presentation**:
- **Path A** (approval): nothing for the maker to do — wait for the
  approver; after approval, return to RunDetail and click Execute again to
  actually broadcast.
- **Path B** (executed): immediately shows `submitted` / `failed` counters
  plus each item's tx hash and on-chain confirmation state.

> 💡 **Why per-item rather than one tx with many outputs?** M1 reuses M0's
> proven single-transfer path (stable, well-exercised) — each employee is a
> separate tx so one failure doesn't take down the others. M2 will introduce
> librustzcash single-tx multi-output Orchard to save fees and avoid leaking
> the recipient count on chain.

### 4.5 Handling partial failures / canceling a run

#### Retry a single failed item

Payroll runs are usually ~95% successful, but occasionally an employee
address typo or insufficient wallet balance produces a partial_success.

- In the RunDetail table, failed items are highlighted in red and show the
  `error_message` (e.g. `invalid recipient address`, `insufficient balance`).
- Click **Retry failed items** → the frontend fans out
  `POST /payroll/runs/{id}/items/{item_id}/retry` per failed item.
- The backend re-runs `transfer_native` only for those items — **already
  confirmed items are not re-sent** (filtered by status).
- After fixing the underlying issue (top up wallet / update employee
  address), a retry usually clears all failures.

#### Cancel a run

`POST /payroll/runs/{id}/cancel` is callable in the following states:

| State | What cancel does |
|---|---|
| `pending` | Direct DB flip to cancelled; nothing on-chain affected. |
| `awaiting_approval` | Same; equivalent to the maker withdrawing. |
| `executing` (stuck) | **Special stuck-recovery path** — for runs left dangling by a backend crash. Already-on-chain items keep their state (cannot be reversed), un-submitted items flip to failed, run flips to cancelled. |

> ⚠️ **Hard rule**: an on-chain confirmed transfer can never be reversed —
> cancel only resets DB state so you can create a new run. To recover an
> already-sent payment you must contact the recipient off-chain.

### 4.6 Reports and archive

In RunDetail click **View Report** at the top:

- Run metadata: pay_period / source wallet / total amount / creator /
  executor / timestamps.
- Item stats: submitted / failed / pending counts.
- Per-item detail: employee + address + amount + status + tx hash +
  on-chain confirmation + failure reason.

Export this report to CSV / PDF (M2 adds an export endpoint; for M1 use the
browser's Save-as-PDF) and file it with the month's finance workpapers.

---

## 5. End-to-end business scenarios

### 5.1 Scenario A: Monthly payroll (most common)

A typical company pays 50 employees per month — ~25 ZEC total, above the
approval threshold. The complete flow spans 3 days:

```
Day -1 (HR prep: 1-2 days before payday)
──────────────────────────────────────────
1. Employees → review the roster
   - New hires enrolled? (single-add / CSV)
   - Departing employees soft-deleted? (active=false)
   - Employees with new wallet addresses edited?
2. Confirm with IT / Finance which wallet is paying this month
   - Sufficient balance? (open wallet detail → check shielded balance)
   - Not enough → top up from the corporate main account → wait 6 blocks

Day 0 (Finance execution: payday)
──────────────────────────────────────────
1. Prepare CSV
   - 4 columns: employee_code, employee_address, amount, memo
   - Match HR's payroll spreadsheet; double-check amounts
2. Payroll Runs → New Run
   - Source wallet: corporate ZEC payroll wallet
   - Pay period: e.g. "2026-05"
   - Upload CSV → 50-row preview appears in the browser instantly
   - See ✅ 50 valid / ❌ 0 invalid → click Create
3. Backend re-validates → run enters pending status
4. Click Execute →
   ─── total 25 ZEC > threshold 20 ZEC ───
   Receive `{result:"awaiting_approval", policy_id, threshold:"20"}`
   1.2 s flash then auto-redirect to /approval/pending
5. Ping the approver (CTO / CFO / another authorized user):
   "Monthly payroll 25 ZEC in the queue, please review"

Day 0 (Approver: within 5 minutes)
──────────────────────────────────────────
1. Approver signs in → Approval Queue → sees this run
2. Click detail → review amount / maker / 50-employee list
3. No anomalies → Approve (note "May 2026 monthly salary")
4. run.status: awaiting_approval → approved
5. System notifies maker (M2 real notification; M1 maker refreshes)

Day 0 (Finance closeout)
──────────────────────────────────────────
1. Back in RunDetail → click Execute again
   ─── this time status is approved, skip the approval check ───
   - Backend calls transfer_native for each of the 50 employees
   - Each item is a real on-chain tx
   - Returns `{result:"executed", submitted:50, failed:0, final_status:"completed"}`
2. RunDetail table shows 50 items all confirmed
3. Spot-check 1-2 employees ("got it") → done
4. Export PDF report into this month's finance workpapers

Day +1 (Audit trail)
──────────────────────────────────────────
If an external auditor is within the scope window they can:
- /auditor/wallets/{id}/transfers shows the 50 outgoing transfers
- Request a disclosure with granularity=range 2026-05-01~2026-05-31 → PDF
- No private key needed; chain data only
```

**Key timing**: do this 3 days before month-end so weekend approver absence
doesn't block. SLA defaults to 24 h but practically aim for 4-8 h.

### 5.1.1 Scenario A contingency: approver SLA timeout

If the approver missed the notification and the SLA (4 h) elapses, the run
auto-expires. Recovery:

- Open My Approvals (maker view) → the monthly run row is now red (expired).
- **You cannot reactivate the old run** (by design — that would defeat the
  SLA concept).
- Create a new run (the same CSV can be reused) → grab the approver in
  person to approve → Execute.

### 5.2 Scenario B: Quarterly audit (auditor onboarding through report)

```
Day 1 (Admin prep)
──────────────────
1. Approval policy    → `wallet=treasury-main / zcash / ZEC / threshold 100`
                        (large-spend approvals leave audit trail)
2. Auditor management → invite auditor@cpa-firm.com
                        scope = [treasury-main, projectA-wallet]
                        window = 2026-01-01 ~ 2026-03-31
                        disclosure budget = 20
                        DM temp_password securely
3. (Optional) Export UFVK on the wallet → DM the `uview...` string
                        (auditor can verify in Zashi independently)

Days 2-30 (Auditor work)
────────────────────────
1. /auditor/login + temp_password → change password
2. Dashboard shows 2 wallets with basic stats
3. WalletDetail shows real on-chain balance + scope-window transfers
4. Request disclosure reports ×N:
   - granularity=tx for a specific transaction in question
   - granularity=range for a full monthly PDF for archive
   - After 20 uses the system refuses new requests
5. File the PDF / CSV reports into the audit workpapers

Day 30 (Admin closeout)
───────────────────────
1. Auditor management → deactivate the auditor account
2. Backend rejects future logins for that account (even with a still-valid
   temp_password)
3. Reports that were already downloaded are unaffected
```

### 5.3 Scenario C: Urgent transfer hit by SLA expiry

```
 09:00  Finance maker submits 50000 USDT transfer (over threshold 1000)
        → status: awaiting_approval / expiry_at: 11:00 (SLA 120 min)
        → maker pings checker on chat: "urgent, please approve"

 09:00 ~ 11:00  checker doesn't see the ping (in a meeting / traveling)

 11:00  SLA worker sweep → flips to "expired"
        → maker sees the row turn red in "My Pending"

 11:01  What does maker do?
        ────────────────────────────────
        ✅ Resubmit: /transfers → enter the same amount → a NEW
           awaiting_approval row is created (the old expired row
           remains as an audit record)
        ❌ Cannot reactivate the expired row (by design — that would
           let users bypass the SLA concept)

 11:02  This time maker grabs the checker in person → approve in 5 min
        → execute → confirmed
```

### 5.4 Scenario D: Partial payroll failure (partial_success handling)

50-person payroll where 3 fail and 47 confirm:

```
Click Execute → wait ~30 s (per-item fan-out)
Receive `{result:"executed", submitted:50, failed:3, final_status:"partial_success"}`

RunDetail table:
┌────┬────────────┬─────────┬───────────┬──────────────────────────────┐
│ id │ employee   │ amount  │ status    │ tx_hash / error              │
├────┼────────────┼─────────┼───────────┼──────────────────────────────┤
│ 1  │ E001 Wang  │ 0.5 ZEC │ confirmed │ 0xabc...                     │
│ 2  │ E002 Li    │ 0.5 ZEC │ confirmed │ 0xdef...                     │
│ 3  │ E003 Zhang │ 0.5 ZEC │ ⚠ failed  │ invalid recipient address    │
│ 4  │ E004 Zhou  │ 0.5 ZEC │ confirmed │ 0xghi...                     │
│ ...│ ...        │ ...     │ ...       │ ...                          │
│ 27 │ E027 Han   │ 0.5 ZEC │ ⚠ failed  │ insufficient balance         │
│ 35 │ E035 Huang │ 0.5 ZEC │ ⚠ failed  │ insufficient balance         │
│ ...│ ...        │ ...     │ ...       │ ...                          │
└────┴────────────┴─────────┴───────────┴──────────────────────────────┘

Diagnosis:
- E003: address typo → ask employee for new address → Employees → edit E003 → back to RunDetail
- E027 / E035: wallet ran out (after a few items the remaining balance
  couldn't cover fee + amount) → top up wallet

Fix:
1. Top up wallet 5 ZEC, wait 6 blocks
2. Update E003 wallet address
3. RunDetail → click Retry N failed
   - Frontend fans out 3 independent retry requests
   - Only failed rows are retried (confirmed rows untouched)
4. ~5 s later → 3 items on chain → run.status auto-flips
   partial_success → completed
```

**Why don't we pre-deduct the balance to prevent insufficient?** Design trade-off:
- Pre-deduction needs wallet locking, serializing against other single
  transfers — scalability hit.
- M1 chose best-effort + retry: surface exactly which items failed and let
  you fix surgically.
- M2 will add a dry-run that estimates total fee + checks balance, surfacing
  "short by N ZEC" before partial execution.

### 5.5 Scenario E: Payroll to ETH wallets (multi-chain)

Payroll is not ZEC-only — for USDT-denominated salaries to overseas employees:

```
1. Employees: set chain=ethereum for overseas staff, address 0x...
2. New Run: source wallet = corporate USDT wallet (chain=ethereum)
3. CSV: amounts in USDT (e.g. amount=2000 means 2000 USDT)
4. Execute → each USDT ERC20 transfer routes through chain_client.transfer_token
5. ETH gas is paid by the source wallet (confirm it holds a small ETH balance)
```

⚠️ Chain + token are determined by the source wallet; **a single run cannot
mix chains** (all ZEC or all USDT, not both). Cross-chain payroll requires
two separate runs.

---

## 6. Troubleshooting and FAQ

### 6.1 Login

| Symptom | Likely cause | Fix |
|---|---|---|
| "Invalid username or password" | Wrong password | Ask ops for the `.env.secrets` initial password |
| "Rate limit exceeded" | ≥ 5 failed attempts in 1 minute | Wait one minute and try again |
| Token expires in 5 min during sensitive ops | Some operations require fresh re-auth | Sign in again |

### 6.2 Approvals

| Symptom | Cause | Fix |
|---|---|---|
| Cannot see my own transfer in "Approval Queue" | By design (maker ≠ checker) | Have a different user act as checker |
| Approve returns 403 | You are the maker, cannot self-approve | Same as above |
| Reject says "reason ≥ 5 characters required" | Mandatory; prevents blank rejections | Provide a meaningful reason |
| Transfer stuck in awaiting_approval | No one approved → eventually flips to expired | Have the checker act, or wait for expiry and resubmit |

### 6.3 Auditor

| Symptom | Cause | Fix |
|---|---|---|
| Admin sidebar has no "Auditor View" entry | By design (dual JWT physical isolation) | Sign out → log in via /auditor/login with the auditor account |
| /auditor/wallets returns 401 "Invalid or expired auditor token" | Admin token cannot access auditor routes | Same as above |
| Disclosure download returns 410 Gone | The one-time token has been consumed | Admin re-exports the viewing key |
| Disclosure request rejected with "budget exhausted" | The quota is used up | Admin increases `max_disclosure_count` |
| Zashi rejects the imported UFVK | Copy/paste truncation | Re-export — UFVK is a single complete `uview1...` line |

### 6.4 Payroll (see chapter 4)

| Symptom | Cause | Fix |
|---|---|---|
| Execute Run returns `awaiting_approval` | Run total triggered an approval policy | Visit /approval/pending and wait for approval, or lower the threshold |
| Run stuck in `executing` | Backend crashed mid-execute | Cancel from executing (already-confirmed items remain; un-submitted ones flip to failed) |
| Single item fails with "insufficient balance" | Wallet balance ran out | Top up the wallet and retry the failed items |

### 6.5 Deployment / ops

| Symptom | Cause | Fix |
|---|---|---|
| Disclosure range fails with "Zcash RPC 401" | Zebra container restart rotated the cookie | Ops re-reads the cookie into .env and `pm2 restart --update-env` |
| Browser keeps loading stale JS after a deploy | index.html cached by an intermediate proxy | Hard reload (Ctrl+Shift+R); nginx config must set `no-cache, no-store` |
| Admin password changed after backend restart | `.env.secrets` was deleted → backend regenerated everything | **Never delete .env.secrets**. If deleted, all encrypted wallets are permanently unrecoverable (warned in M0) |

---

## 7. Security guidance and pitch points

### 7.1 The five questions customers ask most (sales talking points)

> Joint section covering deployment / multi-chain / staging-demo talking points.

1. **Private key safety**
   - Answer: AES-256-GCM encrypted at rest, export requires re-entering the password, every export is logged.
   - Vs. competitors: Gnosis Safe requires every signer to manage their own private key; we centralize storage + add dual-signature + audit log.

2. **How does the auditor verify without a private key?**
   - Answer: Zcash native OVK/IVK/UFVK are read-only keys — handing them out grants visibility, not control.
   - We export as ZIP-316 standard strings so the auditor can import directly into Zashi (open-source wallet) and verify independently.

3. **How do you prevent abuse of approval-threshold bypass?**
   - Answer: `maker ≠ checker` is a database-level constraint, not a frontend RBAC check; reject mandates a reason ≥ 5 chars; SLA auto-expires so a stalled approval doesn't lock the maker forever.

4. **What granularities can be audited?**
   - Answer: single transaction / single address full history / date range. Output in PDF + CSV + JSON. Range supports timestamps which the system auto-resolves to block heights (the auditor doesn't need to know blockchain).

5. **Multi-chain support?**
   - Answer: M1 supports ETH (ERC20) + ZEC (transparent + Orchard shielded). The architecture abstracts to a `ChainClient` trait; adding a new chain only requires implementing the trait interface.

6. **Development cadence and time-to-production**
   - Answer: M1's three business pillars (F1.1 audit & compliance / F2.1 maker-checker / F3.1 bulk payroll) shipped in a single overnight on 2026-05-16~17, with an e2e smoke harness (11 steps / 34 assertions) you can run on every commit. Your customer acceptance timeline is predictable: 1 day in staging → 1 day in-browser testing → 1 day in production.

7. **How does a customer trial / POC?**
   - Answer: <https://staging.example.com> is our reference staging. A customer can stand up the same on their own server in ~30 minutes (see [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md)).
   - A few `apt` + a few `docker` commands → letsencrypt auto-issues HTTPS → demo in the browser.
   - Customer data stays on their own machine — nothing leaves their premises.

8. **One-stop vs assembling a tool-chain**
   - Answer: traditional approach = wallet (MetaMask / Zashi) + multisig (Gnosis Safe) + payroll scripts (Bash / Python hardcoded) + audit exports (wallet screenshots / third-party block explorers + manual Excel) → fragmented chain of trust, scattered audit trail.
   - zPay Enterprise: single sign-on / single audit trail / single RBAC / single wallet encryption layer — **on top of the customer's existing multi-chain wallet capability, add the three enterprise must-haves, and you have a one-stop enterprise Web3 finance platform**.

9. **Privacy vs transparency balance**
   - Answer: Zcash Orchard is shielded by default (chain observers see no recipient, no amount, no memo). When you want a specific auditor to see it, **viewing keys give you self-service control** — no private key sharing, no transactions issued, only read access exported.
   - Transparent chains (ETH etc.) are public by default — actually they need zPay's RBAC + dual-signature layer to guard against mistakes.
   - Customers can **mix per scenario**: sensitive payroll via ZEC + viewing-key audit; operational transfers via ETH + dual-signature.

10. **Demoability (5-minute sales walkthrough)**
    - Create a ZEC wallet → configure approval policy threshold 0.01 → enroll 1 employee → pay 0.05 → triggers awaiting_approval → approve in a second browser window → execute → confirm on chain → export viewing key to the customer's "auditor" account → that account independently sees the transaction and downloads a PDF report — five minutes end-to-end, covers all three business pillars.

### 7.2 Secure-deployment checklist (ops sign-off)

```
□ Admin initial password changed to a strong one (≥ 12 chars); .env.secrets
  backed up but no longer sensitive
□ letsencrypt auto-renewal configured (certbot --nginx -d your-domain)
□ nginx serves index.html no-cache and /assets/ immutable (avoid stale-bundle incidents)
□ MySQL volume mounted on encrypted partition; off-site mysqldump backups on schedule
□ Zebra mainnet RPC cookie written to .env 0600 (backend-readable only)
□ Firewall: 80/443 public; 8090 (backend) / 3307 (mysql) / 8232 (rpc) loopback-only
□ Monitoring: pm2 logs zpay-staging-backend + grafana / promtail tailing RUST_LOG
□ Disaster recovery: wallet .env.secrets backed up to ≥ 2 off-site encrypted stores;
  losing it = historical wallets permanently unrecoverable
□ Business safety: at least one global approval policy with a sensible threshold
  to guard against unaudited initiation
```

### 7.3 Maintenance cadence

- **Monthly**: reconcile employee roster (new hires + departures); review approval-policy coverage.
- **Quarterly**: renew auditor scope windows; deactivate departed auditors.
- **Semi-annually**: confirm letsencrypt auto-renew; re-run `./e2e/smoke.sh` (expect 34/34).
- **Annually**: rotate auditor credentials; force password reset on all active auditors.

---

## 8. Ironwood migration and batch privacy transfers (F4)

> Audience: finance lead / treasury operator. Requires the **admin** role — both features move treasury funds.

### 8.1 Why you are seeing a migration banner

Zcash's **NU6.3** network upgrade (mainnet activation 2026-07-28) opens a new shielded pool called **Ironwood** with a pinned verification mechanism, and closes the legacy Orchard pool to new deposits and in-pool transfers. Existing funds are never at risk: they remain spendable **forward** — each legacy note can cross a one-way *turnstile* into Ironwood. (Independent audits and an external formal-verification effort for the new circuit are in progress; do not read "pinned" as "formally verified".)

What this means for your treasury:

- **Before migrating**: your shielded balance still shows and is safe, but shielded transfers in/out of the legacy pool no longer work after activation.
- **After migrating**: everything works as before — same addresses, same keys, same workflows. Only the underlying pool changed.
- Each migration batch pays a normal network fee. Nothing else is deducted.

### 8.2 Migrating a wallet (wizard)

1. Open the Zcash wallet page. Wallets still holding legacy-pool funds show an advisory banner → click **Migrate**.
2. **Step 1 — About**: what the migration does; confirm the wallet.
3. **Step 2 — Mode**:
   - **Private (recommended)** — splits the balance into several randomly-sized batches spread over a time window with jitter (defaults: 6 batches / 48 h). This avoids publishing an obvious "one company moved its whole treasury at 14:02" fingerprint on the turnstile. Batch amounts are randomized precisely so equal-sized chunks don't link to each other; note that migration amounts are the one thing the turnstile reveals, so privacy here is about reducing linkability — not a guarantee of unlinkability.
   - **Immediate** — a single batch, right now. Use for small balances or when time matters more than discretion.
4. **Step 3 — Confirm**: review the plan (per-batch amounts + schedule) and create the run.
5. Press **Execute** (immediate) or **Start schedule** (private). If the total crosses an approval policy threshold, the run pivots to *awaiting approval* — a **second admin** must approve (the creator cannot approve their own run; this is enforced in the database, not just the UI). One approval covers the whole window; batches then execute unattended.
6. Track progress on the run page: per-batch status, tx hash, and error text if a batch fails. **Failed batches never block siblings** — retry them individually with one click.
7. **Cancel** is the only way to stop remaining batches. Batches already submitted are on-chain and cannot be recalled.

A service restart (upgrade, reboot) is harmless: the schedule lives in the database and resumes exactly where it left off.

### 8.3 Batch privacy transfers (arbitrary recipients)

Generalizes payroll to **any recipient list** — vendors, rebates, grants — as shielded ZEC, with optional privacy scheduling.

1. Sidebar → **Batch Transfers** → **New Batch Transfer**.
2. Fill title + source wallet, then upload a CSV with columns:

   ```
   recipient_address, amount, memo
   utest1abc...,      1.25,   invoice-001
   ```

   - Header row optional. Memo optional (≤ 512 bytes).
   - **Every recipient must be an Orchard-capable unified address** (`u1...`). Transparent or Sapling-only addresses are rejected — this feature is shielded-only by design; use the regular transfer page for transparent payouts.
   - The server validates every row and returns **all** errors at once (bad address, non-positive amount, duplicate rows, total exceeding spendable balance), mapped back to your CSV lines. Fix and re-upload in one pass.
3. **Privacy scheduling**:
   - **Off** — all transfers queue immediately.
   - **Staggered** — transfers are shuffled and spread over N batches across a time window (defaults: 4 / 24 h), so payout timing doesn't correlate the whole batch. An optional **per-transfer cap** splits any row above the cap into several randomly-sized smaller transfers.
4. Approval, execution, progress, per-item retry and cancel behave exactly like migrations (§8.2 steps 5–7). Amounts are **never silently reduced**: if the balance can't cover a payment, that item fails with the node's real error and can be retried after topping up.

### 8.4 Two things about shielding that surprise people

Both are protocol behaviour, not product defects. They apply from NU6.3, when
shielding routes new value into Ironwood.

**1. Shielding spends whole coins, and the remainder comes back shielded.**
Ask to shield 2 ZEC out of a transparent balance made of one 3.125 ZEC coin and
the whole coin is consumed: 2 ZEC lands where you asked, and the remaining
~1.125 ZEC returns to *your own wallet* as a second shielded note. Nothing is
lost — the balance moved from transparent to shielded. The alternative, paying
change back to the transparent address, would publish "this address just spent
and has this much left" on-chain and give away half of what shielding buys you.

**2. The shielding fee is charged per coin consumed, not per amount.**
One coin costs 15,000 zatoshis; ten small coins cost 60,000 for the same total
value. This is the ZIP-317 rule (each transparent input is a logical action), so
a wallet topped up by many small deposits is more expensive to shield than one
funded by a single large payment. If the balance cannot cover the fee for the
coins selected, the transfer is refused up front with both numbers rather than
being under-paid and rejected by the network.

### 8.5 FAQ / troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Shielded more than asked | Shielding spends whole coins (§8.4) | Expected — the remainder is back in your wallet as a shielded note |
| Shielding fee higher than a colleague's for the same amount | Fee is per coin consumed (§8.4) | Expected — consolidate deposits if fee predictability matters |
| Banner says legacy funds but balance looks normal | Expected — the banner reflects pool membership, not solvency | Migrate at your convenience before you next need shielded transfers |
| "wallet already has migration run #N in state ..." | One active migration per wallet, by design | Finish or cancel the active run first |
| Execute returns *awaiting approval* | Run total crossed an approval policy | A second admin approves (maker self-approval returns 403) |
| CSV rejected with row errors | Address/amount/duplicate problems | All rows are reported at once — fix the file and re-upload |
| A batch/item shows *failed* with an error | Node rejected or balance insufficient at execution time | Read the stored error text (it is the node's verbatim reply), fix cause, click retry |
| Backend restarted mid-window | Nothing lost | Scheduler is database-driven; remaining batches fire on time |

---

(This document is actively maintained.)

**Last updated**: 2026-07-24

**Feedback**: please file a repository issue.
