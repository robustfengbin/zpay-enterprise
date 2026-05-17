[English](README.md) | [中文](README_CN.md)

# zPay Enterprise

> **Privacy-first financial operating system for Web3.**
> Custodial multi-chain wallet · Maker-checker treasury controls · Bulk payroll · Regulator-grade audit — all in one self-hosted service.

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-v0.3.0-green.svg)](CHANGELOG.md)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Contributions welcome](https://img.shields.io/badge/contributions-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Security policy](https://img.shields.io/badge/security-disclosure-red.svg)](SECURITY.md)

zPay Enterprise lets a company move money on public blockchains with the same privacy, security, and control as a traditional bank treasury — but without a custodian, a multisig setup ritual, or a bespoke audit pipeline. **One self-hosted service** covers wallet custody (Ethereum + Zcash Orchard shielded), policy-driven dual-signature approvals, bulk payroll, and on-demand disclosure reports for external auditors.

![Payroll run awaiting approval — F2.1 threshold hook auto-triggers on the batch total](docs/images/m1-payroll-runs.png)

---

## 🎉 What's New in v0.3.0 — M1 Enterprise (June 2026)

The first M1 release. Three business pillars added on top of the M0 multi-chain wallet core, plus end-to-end smoke coverage and bilingual operations docs. **All existing M0 callers continue to work unchanged.**

### 🔐 F1.1 — Viewing-Key Audit & Disclosure
Dedicated **Auditor role** with its own login and a separate JWT (`kind=auditor`), so a leaked admin token can never reach an audit endpoint and vice versa. One-click export of **OVK / IVK / UFVK** — the UFVK is a ZIP-316 standard `uview...` string that pastes directly into Zashi or any compatible viewing-only wallet. On-demand **ZIP-307 inspired disclosure reports** in PDF / CSV / JSON, scoped to a wallet + time window + per-quarter budget. → [PRD-F1.1](docs/public/PRD-F1.1.md)

![Auditor management — invite scoped third-party auditors](docs/images/m1-manage-auditors.png)

### 🛡 F2.1 — Maker-Checker Approvals
Configurable **approval policies** along (`scope × chain × token × amount × SLA`). `maker ≠ checker` is enforced **at the SQL layer**, not just the frontend. Auto-pivot on `POST /transfers` when the amount meets a policy; reject requires a written reason (≥ 5 chars); a **5-minute SLA worker** auto-expires stalled requests so they don't block the maker forever. → [PRD-F2.1](docs/public/PRD-F2.1.md)

![Approval policies — enterprise-grade thresholds per chain × token](docs/images/m1-approval-policies.png)

### 💰 F3.1 — Bulk Payroll
Employee roster with CSV import + **two-stage validation** (client + server). Per-item Orchard fan-out (real on-chain). The F2.1 threshold hook means a large monthly run goes through **one approval**, not N. Partial failures retry individually; runs stuck in `executing` can be force-cancelled as a recovery path. → [PRD-F3.1](docs/public/PRD-F3.1.md)

![Employee roster — 6 English-named demo employees across multiple chains](docs/images/m1-employees.png)

### 📐 Operations & developer experience
- **End-to-end smoke harness** — 11 steps × 34 assertions in one shell script (when bundled).
- **Bilingual user manual** — [English](docs/USER-MANUAL-EN.md) · [中文](docs/USER-MANUAL-CN.md), organized by **role and business scenario**, not by feature.
- **30-minute staging recipe** — [STAGING-DEPLOYMENT.md](docs/STAGING-DEPLOYMENT.md) takes a fresh Linux host to a live HTTPS deployment.

Full per-area change list: [CHANGELOG.md](CHANGELOG.md).

---

## 🎯 Who Uses zPay

zPay is built for the four people who actually move money in a Web3 company:

### CFO / Finance Director
*"I need to move ZEC and USDT to vendors and employees every month. I don't want any single person to be able to walk away with funds, and the auditor needs to see what we did at quarter-end."*

→ Pick a maker-checker policy for spend above your threshold, run monthly bulk payroll with a CSV upload, hand the auditor a scoped read-only login at quarter-end. **No private keys ever leave your server.**

### Treasury / Operations Manager
*"I write the policy, but I'm not the one approving every transaction. I want a dashboard that tells me who's pending what, how close we are to SLA, and which payroll runs need attention."*

→ Configure approval policies along `chain × token × threshold × SLA`. Watch the queue. Force-cancel stuck runs. Adjust thresholds as the team grows — backend is forward-only and stays in sync.

### External Auditor (Big-4 / regional CPA)
*"My client gave me a login. I need to verify on-chain history for Q1 without seeing private keys, and I need to walk away with PDFs for my workpapers."*

→ Log into a separate `/auditor/login` portal (admin tokens are blocked here). See exactly the wallets the client granted you, only within the granted window. Generate disclosure PDFs that anchor each transaction to a revealed nullifier — independently verifiable on the Zcash chain.

### DevOps / Security Lead
*"I need to deploy this without becoming a key-custody startup. I want one HTTPS service, encrypted at rest, automated tests, and a clear story for incident response."*

→ Single Rust binary + MySQL + Zebra full-node (Docker). AES-256-GCM at rest. Dual-JWT isolation. `e2e/smoke.sh` covers every M1 path. Letsencrypt auto-renew via the [staging recipe](docs/STAGING-DEPLOYMENT.md). [Security policy](SECURITY.md) for vulnerability disclosure.

---

## ✨ Core Capabilities

### Multi-chain wallet custody
- **Ethereum** (native + ERC-20: USDT / USDC / DAI / WETH) and **Zcash** (transparent + Orchard shielded).
- Private keys encrypted at rest with **AES-256-GCM** — they never sit as plaintext on disk and require a re-entered admin password to export.
- Configurable RPC endpoints with fallback support; EIP-1559 gas estimation.
- Extensible: implement the `ChainClient` trait to add a new chain.

### Zcash Orchard privacy
- All **four transfer modes**: T→T, T→Z (shielding), Z→Z (full privacy), Z→T (de-shielding).
- **Halo 2** zero-knowledge proofs with no trusted setup.
- ZIP-317 fee structure; ZIP-316 unified address parsing; ZIP-307 inspired disclosure body.
- Background Orchard sync with per-wallet progress tracking.

### Enterprise treasury controls (M1)
- **Approval policies** scope × chain × token × threshold × SLA. Matching is most-specific-first.
- `maker ≠ checker` SQL-layer enforcement so a frontend RBAC bug cannot bypass.
- **SLA worker** flips overdue `awaiting_approval` rows every 5 minutes — stalled approvals can never block forever.
- **Bulk payroll** with CSV upload, two-stage validation, per-item retry, force-cancel for stuck runs.

### Audit & compliance (M1)
- **Independent Auditor role** with separate JWT (`kind=auditor`) — dual-JWT physical isolation between admin and auditor surfaces.
- **Per-wallet scope + time window + disclosure budget** so an auditor sees only what was granted, only when, and only N times.
- **Viewing-key export** as ZIP-316 standard `uview...` string — auditors verify independently in Zashi.
- **Disclosure reports** in PDF, CSV, or JSON. Range scope accepts ISO 8601 timestamps which the server resolves to block heights automatically.

### Internationalization
- All UI strings are i18n-keyed. English and Chinese ship in `frontend/src/locales/`.

---

## 🛡 Security & Compliance Highlights

| Area | Implementation |
|---|---|
| **Private-key storage** | AES-256-GCM encrypted at rest, never plaintext on disk |
| **Key export** | Re-entered admin password required (fresh-intent), full audit log even if download token is consumed |
| **JWT isolation** | Admin JWT (`kind=user`) and Auditor JWT (`kind=auditor`) — SQL-level routing means neither can touch the other's endpoints |
| **Approval enforcement** | `WHERE initiated_by <> viewer_user_id` is the database constraint, not just a frontend check |
| **Reject hygiene** | Approver must provide a reason ≥ 5 characters before a reject lands |
| **SLA hygiene** | A 5-minute background worker flips stalled `awaiting_approval` rows to `expired`, preventing permanent block |
| **CORS** | Explicit `ALLOWED_ORIGIN` allowlist; wildcards are rejected by the service on boot |
| **Rate limiting** | `/auth/login` is governor-limited per peer IP |
| **Disclosure traceability** | Each disclosure entry includes a **revealed nullifier** that anchors it to the Zcash chain — auditors can independently verify without trusting our backend |
| **Forward-only migrations** | Schema additions are nullable; existing M0 callers never break on upgrade |

For full vulnerability-disclosure policy, see [SECURITY.md](SECURITY.md).

---

## 🚀 5-Minute Quick Start (Docker)

```bash
git clone https://github.com/robustfengbin/zpay-enterprise.git
cd zpay-enterprise
cp backend/.env.example .env
docker compose up --build
```

- Backend boots on `http://localhost:8080`.
- On first start, missing secrets (`WEB3_SECURITY__ENCRYPTION_KEY`, `WEB3_JWT__SECRET`, `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD`) are auto-generated and written to `backend/.env.secrets` (chmod 0600, gitignored).
- **Back up `backend/.env.secrets`.** Loss of that file = permanent loss of all encrypted wallets.
- In production, set `WEB3_SERVER__ALLOWED_ORIGIN` to the exact origin your frontend serves from. The service refuses to start if it's unset.

Full setup walkthrough: [QUICKSTART.md](QUICKSTART.md) · Production-grade deploy: [STAGING-DEPLOYMENT.md](docs/STAGING-DEPLOYMENT.md)

---

## 📚 Documentation Map

| Document | Audience | Purpose |
|---|---|---|
| [README.md](README.md) (this file) | All | Product overview + release highlights |
| [README_CN.md](README_CN.md) | 中文用户 | 产品概览 + 发布要点（中文版） |
| [QUICKSTART.md](QUICKSTART.md) | Developers | 5-minute local Docker bring-up |
| [docs/STAGING-DEPLOYMENT.md](docs/STAGING-DEPLOYMENT.md) | Ops | Fresh Linux host → live HTTPS deployment (~30 min) |
| [docs/USER-MANUAL-EN.md](docs/USER-MANUAL-EN.md) | CFO / auditor / operator | End-to-end operations by role and business scenario |
| [docs/USER-MANUAL-CN.md](docs/USER-MANUAL-CN.md) | 财务 / 审计师 / 操作员 | 按角色 + 业务场景组织的中文操作手册 |
| [docs/public/PRD-F1.1.md](docs/public/PRD-F1.1.md) | Engineers / partners | Viewing-key audit + disclosure spec |
| [docs/public/PRD-F2.1.md](docs/public/PRD-F2.1.md) | Engineers / partners | Maker-checker approvals spec |
| [docs/public/PRD-F3.1.md](docs/public/PRD-F3.1.md) | Engineers / partners | Bulk payroll spec |
| [docs/product-roadmap-2026.md](docs/product-roadmap-2026.md) | Investors / customers | Annual roadmap (EN) |
| [docs/product-roadmap-2026-cn.md](docs/product-roadmap-2026-cn.md) | 投资人 / 客户 | 年度路线图（中文）|
| [docs/zcash-enterprise-use-cases-en.md](docs/zcash-enterprise-use-cases-en.md) | Sales | Eight detailed enterprise use cases |
| [docs/orchard_privacy_transfer_architecture.md](docs/orchard_privacy_transfer_architecture.md) | Engineers | Orchard sync / witness / fan-out internals |
| [CHANGELOG.md](CHANGELOG.md) | All | Per-release detailed change list |
| [SECURITY.md](SECURITY.md) | Security researchers | Vulnerability disclosure policy |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Contributors | Dev setup, code style, PR guidelines |

---

## 🛣 Roadmap (2026)

We are building the **enterprise-grade privacy finance infrastructure for Web3** — the first platform that lets companies move money on public blockchains with the same privacy, security, and control as a traditional bank treasury.

| Quarter | Theme | Key deliverables |
|---|---|---|
| **Q1** | Enterprise Reliability | End-to-end transaction tracking, auto failover, real-time dashboards (M0 baseline) |
| **Q2** | Compliance & Governance | **Maker-checker, audit roles, ZIP-307 disclosures, bulk payroll** — v0.3.0 M1 (shipped this release) |
| **Q3** | High-Volume Privacy | Optimized Orchard sync, multi-output single-tx fan-out, large-value transfers, unified balance management |
| **Q4** | Privacy Finance Platform | Developer SDKs, multi-chain treasury, HSM / KMS integrations, webhook fan-out |

Full vision document: [docs/product-roadmap-2026.md](docs/product-roadmap-2026.md) · [中文版](docs/product-roadmap-2026-cn.md)

---

## 🧩 Tech Stack

**Backend** — Rust · Actix-web 4 · SQLx (MySQL 8) · librustzcash (Orchard 0.13 / Halo 2) · ethers-rs · printpdf · AES-256-GCM · JWT

**Frontend** — React 19 · TypeScript · Vite · Tailwind CSS · i18next · React Router 7

**Operations** — Docker Compose · PM2 · nginx (SPA cache + reverse proxy) · letsencrypt (certbot)

---

## 🎯 Enterprise Use Cases (Eight Detailed Scenarios)

zPay maps to specific enterprise workflows — full details with API examples in the dedicated use cases guide:

1. **Cryptocurrency payment gateway** — e-commerce ZEC + USDT acceptance with privacy
2. **Corporate treasury** — separation-of-duties + audit logs + multi-signature workflow
3. **OTC trading desk** — confidential large-value Z→Z trades + privacy for counterparties
4. **Privacy-focused exchange** — shielded customer deposits + automated balance reconciliation
5. **Cross-border remittance** — multi-chain settlement (ETH for speed, ZEC for privacy)
6. **Institutional custody** — per-client wallet segregation + view-only keys for auditors
7. **Supply-chain finance** — confidential vendor payments with selective regulator disclosure
8. **Payroll distribution** — batch bulk payroll with policy-gated approvals (M1 ✅)

→ [Full use cases guide (English)](docs/zcash-enterprise-use-cases-en.md) · [中文版](docs/zcash-enterprise-use-cases.md)

---

## 🤝 Contributing

Bug reports, feature ideas, and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and submission guidelines.

Quick version:

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes
4. Push to the branch and open a Pull Request

If you find this project useful, consider supporting development:

**ETH / USDT / USDC (ERC20):** `0xD76f061DaEcfC3ddaD7902A8Ff7c47FC68b3Dc49`

---

## 🙏 Acknowledgements

Built on [Zcash Orchard](https://github.com/zcash/orchard) + [Halo 2](https://github.com/zcash/halo2), [Ethers-rs](https://github.com/gakonst/ethers-rs), and [Actix Web](https://actix.rs).

---

## 📄 License

Released under the [Apache License 2.0](LICENSE).

For security vulnerability disclosure see [SECURITY.md](SECURITY.md) — do not open a public issue for security reports.
