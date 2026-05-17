# zPay Enterprise — User Operations Manual

> End-to-end operations guide for finance, compliance, and audit roles.
> Staging companion: <https://zpaystage.fastaitop.com>
> For technical detail see the [PRD docs](.) and [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md).
> 中文版本: [USER-MANUAL-CN.md](USER-MANUAL-CN.md)

---

## Contents

- [1. Document map and role definitions](#1-document-map-and-role-definitions)
- [2. First-time setup after deployment](#2-first-time-setup-after-deployment)
- [3. Audit and compliance](#3-audit-and-compliance)
- [4. Employees and bulk payroll](#4-employees-and-bulk-payroll) (sweden — to be filled)
- [5. End-to-end business scenarios](#5-end-to-end-business-scenarios)
- [6. Troubleshooting and FAQ](#6-troubleshooting-and-faq)
- [7. Security guidance and pitch points](#7-security-guidance-and-pitch-points)

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

Once deployment is done, visit <https://zpaystage.fastaitop.com> (or your domain) and use the login page:

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

1. Visit <https://zpaystage.fastaitop.com/auditor/login> (**note the URL is different** — separate entry).
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

> sweden chapter — to be filled. Covers:
> - Employee roster management (manual add / CSV import / soft delete)
> - Bulk payroll lifecycle (New Run → two-stage validate → Execute → tagged-union outcome)
> - F2.1 threshold hook bridging into payroll (run total triggers approval)
> - Partial-failure single-item retry + cancel from any state
> - Full demo walkthrough

---

## 5. End-to-end business scenarios

### 5.1 Scenario A: Monthly payroll (most common)

> sweden chapter — to be filled

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

### 5.4 Scenario D: Partial payroll failure (see sweden's chapter 5)

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

### 6.4 Payroll (detail in sweden's chapter)

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

> Joint section, sweden will add deployment/multi-chain/staging-demo talking points.

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

---

(Chapters 4, 5.1, 5.4, and 7 are to be filled by sweden. This document is actively maintained.)

**Last updated**: 2026-05-17 by france 🥖 + sweden 👑

**Feedback**: please file a GitLab issue or post in the team Discord channel.
