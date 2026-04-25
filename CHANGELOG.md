# Changelog

All notable changes to zpay-enterprise are documented in this file.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project aims to follow [Semantic Versioning](https://semver.org/).

---

## [0.2.0] — 2026-04-25 — "Housekeeping Release"

First release after a single-evening housekeeping sprint that turned a
repo which "looked alive but couldn't be built by strangers" into one
with a working `docker compose up --build`, auditable defaults, and a
license.

### ⚠️ Breaking changes

- **Default admin credentials removed.** The `admin` / `admin123` pair
  that was hard-coded into `create_default_admin` and printed in both
  READMEs is gone. On first startup the service now requires
  `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD` to be set (at least 12
  characters, cannot be `admin123`), or — if that variable is empty —
  auto-generates a strong random password, writes it to
  `backend/.env.secrets`, and logs a loud warning.

  **Upgrading a live deployment:** the pre-existing admin row in your
  database is *not* touched (the default-admin logic is idempotent), so
  its password is still whatever you set previously. To keep the service
  starting after upgrade you still need to supply a non-weak value for
  `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD` (it simply won't be used for
  the existing row). To rotate the live admin password, log in and use
  **Settings → Change Password**, or update
  `users.password_hash` directly in MySQL.

### Added

- **LICENSE** — the project is now released under
  [Apache License 2.0](LICENSE). Previously no license file was
  present, which meant "all rights reserved" by default and blocked
  downstream use, forks, and packaging.
- **SECURITY.md** — security policy, private-disclosure contact,
  supported versions, and a hardening checklist for operators.
- **CONTRIBUTING.md** — contribution workflow, dev setup notes, and
  coding-style expectations.
- **Dockerfile** — two-stage build (`rust:1.90-bookworm` → `debian:bookworm-slim`),
  non-root runtime user (uid 10001), compiles the backend in debug mode
  per the project's `CLAUDE.md` rules.
- **docker-compose.yml** — one-command quick-start that bundles MySQL 8
  for first-run convenience, wires up all required `WEB3_*` environment
  variables, and uses `${VAR:?err}` so the stack fails fast when a
  required secret is missing.
- **.dockerignore** — keeps `target/`, `node_modules`, `.env`,
  local logs, and the `ai_note/` planning directory out of the build
  context.
- **Automatic secret generation.** If any of
  `WEB3_SECURITY__ENCRYPTION_KEY` (32 chars, AES-256 key),
  `WEB3_JWT__SECRET`, or `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD` is
  empty on first start, the backend generates a strong random value and
  persists it to `backend/.env.secrets` (mode `0600` where possible).
  Existing values are **never rotated** on restart — rotating the
  AES key would render every previously encrypted wallet unrecoverable.
- **QUICKSTART.md** — five-minute getting-started guide from
  `git clone` to a successful API login.
- **`backend/Cargo.lock` is now committed.** `zpay-enterprise` is a
  binary crate, so the lockfile belongs in version control — this gives
  reproducible builds across contributors and makes future yank events
  (like the one that motivated this release) visible in CI rather than
  in a stranger's laptop.

### Changed

- **Zcash library stack upgraded** so `cargo build` works again:

  | crate | before | after |
  | --- | --- | --- |
  | `orchard` | 0.11 | **0.13** |
  | `zcash_primitives` | 0.26 | **0.27** |
  | `zcash_protocol` | 0.7 | **0.8** |
  | `zcash_address` | 0.10 | **0.11** |
  | `zip32` | — | **0.2** (newly split out of `zcash_primitives`) |

  **Why:** every version of `core2 = ^0.3` / `0.4.0` was yanked from
  crates.io earlier in 2026, which blocked fresh builds of every
  project still pinned to `orchard 0.11`/`0.12` and `zcash_address 0.10`
  (they all depend on `core2`). The 0.27-era Zcash stack moved to the
  `corez` fork; upgrading as a group restored reproducible builds.
  Only one source change was required — `use zcash_primitives::zip32`
  became `use zip32` in `backend/src/blockchain/zcash/orchard/keys.rs`.

- **Docker builder image** bumped from `rust:1.84-bookworm` to
  `rust:1.90-bookworm` because `zcash_encoding 0.4.0` (transitively
  pulled by the new Zcash stack) requires Rust's `edition2024` feature,
  which stabilised in 1.85.1.

- **Frontend login copy** (`en.json`, `zh.json`) no longer advertises
  `admin / admin123`; it now points operators at the env-var-based
  workflow.

- **READMEs** (`README.md`, `README_CN.md`) replaced the "Default
  Credentials → admin / admin123" section with the new env-var-based
  initial-admin procedure, added License / Rust / Contributions /
  Security badges at the top, and a License / Security / Contributing
  / Acknowledgements footer at the bottom.

### Fixed

- **Fresh `cargo build` / `docker build` now succeed.** The root cause
  was the yanked `core2` transitive dependency via `orchard 0.11`
  and `zcash_address 0.10`. See "Changed → Zcash library stack
  upgraded" above.

### Security

- **Node-operator advisory in SECURITY.md.** On 2026-04-17 the Zcash
  Foundation disclosed four vulnerabilities in `zebrad` / `zcashd`:
  - CVE-2026-34377 — Zebra V5 transaction cache bypass, can cause
    consensus split (High)
  - CVE-2026-40881 — addr/addrv2 deserialization DoS (Moderate)
  - CVE-2026-34202 — crafted V5 transaction panic DoS
  - CVE-2026-40880 — cached mempool verification bypass

  None of these affect zpay-enterprise itself (we're an RPC client, we
  don't execute Zcash consensus, P2P networking, or mempool caching),
  but anyone running a `zebrad` / `zcashd` node behind zpay-enterprise
  should be on **Zebra ≥ 4.3.1** or **zcashd ≥ 6.12.1**. This is now
  called out in the Hardening Checklist.

- **`backend/Cargo.lock` committed** (see Added) means future yanked
  transitive dependencies are visible via CI / `cargo audit` rather
  than silently broken for new contributors.

### Known issues

- The Docker image does **not** bundle the React frontend. Build it
  separately with `cd frontend && npm ci && npm run build` and serve
  `frontend/dist/` from any static host.
- The bundled `mysql` service is meant for first-run convenience, not
  production. Point `WEB3_DATABASE__*` at a managed instance for real
  deployments.
- The default `WEB3_ETHEREUM__RPC_URL` uses a public, rate-limited
  endpoint (`https://eth.llamarpc.com`). Replace it with an Alchemy /
  Infura / self-hosted node before relying on it.
- No `/health` endpoint yet; `docker compose ps` and the startup logs
  are the current liveness signals.

---

## Unreleased

Items on deck for the next housekeeping pass. Subject to reprioritisation:

- `/health` and `/metrics` (Prometheus) endpoints
- Optional `zpay-client` crate on crates.io for integrators
- Pre-built Docker image on Docker Hub (`docker run robustfengbin/zpay:latest`)
- Lightwalletd-first Zcash RPC path (skip running a full `zebrad`)
- `scripts/install.sh` to auto-detect host dependencies for
  bare-metal (non-Docker) installs

---

[0.2.0]: https://github.com/robustfengbin/zpay-enterprise/releases/tag/v0.2.0
