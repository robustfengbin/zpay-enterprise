# Security Policy

## Supported Versions

We are in early development. Security fixes are applied to the `main` branch
and to the latest tagged release.

| Version | Supported |
| ------- | --------- |
| `main`  | ✅        |
| `0.x`   | ✅ (latest minor only) |
| Older   | ❌        |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

If you believe you have found a security vulnerability in zpay-enterprise,
please report it to us privately so we can investigate and fix it before it
becomes public knowledge.

**Contact:** security@fastaitop.com

If you prefer encrypted communication, include your PGP public key in your
first message and we will respond in kind.

### What to Include

Please include as much of the following information as you can:

- A short description of the vulnerability
- The component affected (backend module, frontend, dependency, deployment)
- Steps to reproduce the issue
- Proof-of-concept code or a minimal reproducible example, if available
- Your assessment of the impact (confidentiality / integrity / availability)
- Whether you intend to disclose the vulnerability publicly, and if so, when

### Our Commitment

- We will acknowledge receipt of your report within **72 hours**.
- We will provide an initial assessment within **7 days**.
- We will keep you updated on progress toward a fix.
- We will credit you in the release notes of the fix, unless you prefer to
  remain anonymous.
- We will not take legal action against researchers who follow this policy
  in good faith.

### Scope

In scope:

- The backend service (Rust, `backend/`)
- The frontend application (`frontend/`)
- Default configuration and deployment scripts (`start.sh`, `ecosystem.config.cjs`)
- Any first-party SDKs or tools released under this repository

Out of scope:

- Third-party dependencies (please report upstream first; we will help
  coordinate if needed)
- Self-hosted deployments misconfigured by the operator
- Social engineering, physical access, or denial-of-service attacks that
  exceed typical operational protection

## Known Security Considerations

zpay-enterprise is an **early-stage** project. Before deploying to production
with real funds, operators should be aware of the following:

- **Default credentials**: Historical releases shipped with a default
  administrator account. This is being removed. Always verify that no default
  credential is present in your deployment, and that the first-login flow
  forces a password rotation.
- **Private key storage**: Private keys are encrypted at rest with AES-256-GCM
  using a key derived from the operator password. Loss of the operator
  password means irrecoverable loss of funds. Use an external HSM/KMS where
  possible.
- **Self-hosted only**: zpay-enterprise is designed for self-hosted deployment.
  We do not operate a managed service and we do not have access to your keys
  or data.
- **Third-party RPC**: Blockchain RPC endpoints (Alchemy, Infura, QuickNode,
  etc.) observe your transaction traffic. Use private or self-hosted nodes
  for sensitive workloads.

## Hardening Checklist (Operator)

Items marked ✅ ship in the noted version; items marked ☐ are still the
operator's responsibility. Items in *(post-0.2.0)* land in the next
release after the hardening sprint described below.

- [x] LICENSE and SECURITY policy are present and current — *(0.2.0)*
- [x] No default credentials in the running instance — *(0.2.0)*
- [x] Restrictive CORS allowlist (no `allow_any_origin`) — *(post-0.2.0)*
- [x] Rate limiting on authentication endpoints — *(post-0.2.0)*
- [x] Content-Security-Policy header on responses — *(post-0.2.0)*
- [x] Request body size limits — *(post-0.2.0)*
- [ ] Strong, unique operator passwords (16+ chars, random)
- [ ] TLS is terminated in front of the service
- [ ] Database backups are encrypted and tested
- [ ] RBAC is configured (Admin / Operator / Approver / Auditor separation)
- [ ] Audit log shipping to a SIEM or tamper-evident store
- [ ] Regular dependency updates (`cargo audit`, `npm audit`)
- [ ] If self-hosting a Zcash full node for RPC, run **Zebra ≥ 4.3.1**
      or **zcashd ≥ 6.12.1** (2026-04-17 disclosure). Earlier versions
      are vulnerable to consensus split (CVE-2026-34377) and network
      DoS (CVE-2026-40881). zpay-enterprise itself is an RPC client
      and is **not** affected by those node-side bugs, but an outdated
      node can be crashed or forked, taking your wallets offline.

## Hardening Sprint — 2026-04-25

After tagging v0.2.0 we ran an internal review of the codebase and
acted on the findings the same evening. Recording it here so future
contributors and operators know what was looked at.

**Findings closed in this sprint:**

| ID    | Severity | Issue                                                            | Fix                                                                                |
|-------|----------|------------------------------------------------------------------|------------------------------------------------------------------------------------|
| P0-1  | Critical | `Cors::default().allow_any_origin()` on a wallet-custody API     | Replaced with explicit `WEB3_SERVER__ALLOWED_ORIGIN` allowlist; service refuses to start without it |
| P1-1  | High     | No rate limiting on `/api/v1/auth/login`                         | `actix-governor`-style middleware; per-IP throttle on login + global flood ceiling |
| P1-2  | High     | `AuthenticatedUser` extractor returned a synthetic user on missing claims (fail-open) | Returns `Unauthorized` when claims are missing — defence in depth                  |
| P1-3  | High     | No global request body size limit                                | `JsonConfig` and `PayloadConfig` caps wired into `App::new()`                      |
| P1-4  | High     | JWT in `localStorage` exposed to any XSS                         | Short-term: restrictive Content-Security-Policy header on backend responses        |

**Findings deferred (tracked, not addressed yet):**

- **P1-4 medium-term:** move JWT out of `localStorage` into `httpOnly`,
  `Secure`, `SameSite=Strict` cookies, with CSRF tokens for mutating
  requests. Larger frontend change; planned for a future release.
- **P2-1:** stateless JWT, no server-side revocation. Mitigated by
  24-hour expiry; full revocation needs `token_version` claim or a
  short-lived access + DB-tracked refresh model.
- **P2 / P3 items:** see the internal `security-audit.md` report. Will
  be addressed in subsequent hardening rounds.

**Positive findings worth preserving** (do not regress these):

- 0 `unsafe` blocks, 0 `panic!`s, 0 hot-path `unwrap()`s in production code
- All SQL is parameterised through `sqlx`
- `AuthMiddleware` is a single chokepoint covering all protected routes
- Sensitive operations (e.g. private-key export) require admin role
  *and* a password reverification
- Hardcoded credentials present only in test code
- Docker image runs as a dedicated non-root uid (10001)
- `argon2` for password hashing, AES-256-GCM for at-rest encryption

## Disclosure Timeline

We follow responsible disclosure:

1. **Day 0**: Report received, acknowledged.
2. **Day 7**: Initial assessment shared with reporter.
3. **Day 30**: Target fix merged to `main` (complex issues may extend).
4. **Day 30–90**: Coordinated disclosure window with reporter.
5. **Disclosure**: Advisory published via GitHub Security Advisories,
   CVE filed when applicable, release notes updated.

---

Thank you for helping keep zpay-enterprise and its users safe.
