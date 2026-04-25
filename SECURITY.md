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

Before going live, we recommend operators verify:

- [ ] LICENSE and SECURITY policy are present and current
- [ ] No default credentials in the running instance
- [ ] Strong, unique operator passwords (16+ chars, random)
- [ ] TLS is terminated in front of the service
- [ ] Database backups are encrypted and tested
- [ ] RBAC is configured (Admin / Operator / Approver / Auditor separation)
- [ ] Audit log shipping to a SIEM or tamper-evident store
- [ ] Rate limiting on authentication endpoints
- [ ] Regular dependency updates (`cargo audit`, `npm audit`)

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
