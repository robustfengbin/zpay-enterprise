# Contributing to zpay-enterprise

First off, thank you for taking the time to contribute. This guide exists to
help you make a useful contribution with as little friction as possible.

zpay-enterprise is an **enterprise-grade, self-hosted Rust + React stack for
privacy-preserving payment and treasury operations on Zcash, Ethereum, and
beyond**. The project is early-stage — contributions of all sizes are welcome,
from typo fixes to major features.

---

## Code of Conduct

All contributors are expected to uphold our Code of Conduct: treat others
with respect, assume good faith, and keep discussion focused on the work.
Harassment, discrimination, or personal attacks will not be tolerated.

Report violations to `conduct@fastaitop.com`.

---

## How to Contribute

### Reporting a Bug

1. Search [existing issues](https://github.com/robustfengbin/zpay-enterprise/issues)
   first — someone may already have reported it.
2. If not, open a new issue and include:
   - zpay-enterprise version (`git rev-parse HEAD` or release tag)
   - Operating system and Rust toolchain version (`rustc --version`)
   - Steps to reproduce
   - Expected vs. actual behavior
   - Relevant log snippets (trim sensitive data first)

**Do not report security vulnerabilities on the public issue tracker.**
See [SECURITY.md](SECURITY.md) for private disclosure.

### Suggesting a Feature

1. Open a GitHub Discussion (preferred) or a feature-request issue.
2. Explain **the problem** before proposing a solution.
3. If the feature is large, wait for maintainer feedback before writing code —
   this saves you from building the wrong thing.

### Submitting a Pull Request

1. **Fork** the repository and create a branch from `main`:
   ```bash
   git checkout -b feat/short-description
   # or
   git checkout -b fix/short-description
   ```
2. **Make your change**, keeping commits focused and logically grouped.
3. **Run the test suite** locally (see [Development Setup](#development-setup)).
4. **Open a PR** against `main`. Include:
   - A clear description of the change and why it matters
   - Screenshots or terminal output for UI / CLI changes
   - A note on backward compatibility
5. **Address review feedback**. We aim to respond within 48 hours on weekdays.

### Commit Messages

We follow a light Conventional Commits style:

```
<type>: <short summary>

<optional longer body>
```

Common types:
- `feat:` — new user-facing functionality
- `fix:` — bug fix
- `docs:` — documentation only
- `chore:` — tooling, dependencies, build
- `refactor:` — no behavior change
- `test:` — test-only changes
- `perf:` — performance improvement

Example:
```
feat: add Selective Disclosure view-key endpoint

Issues a time-bounded view key scoped to a single account, for use
by auditors and banking counterparties. Returns a serialized key
plus a signed expiry timestamp.
```

---

## Development Setup

### Prerequisites

- **Rust** stable (edition 2021). Install via [rustup](https://rustup.rs/).
- **Node.js** 20+ and npm 10+ for the frontend.
- **MySQL** 5.7+ (or MariaDB 10.x) for the database.
- (Optional) **Docker** if you prefer containerized dev.

### Backend

```bash
cd backend
cp .env.example .env    # fill in DB + secrets
cargo build             # debug build, per project policy
cargo test
./pm2.sh                # or use the start script
```

**Project policy (from `CLAUDE.md`)**:
- Use **debug** build (`cargo build`), not `--release`. PM2 configs point at
  `target/debug/`.
- Table schemas are created / migrated at service startup from Rust code.
  Do not hand-edit SQL migrations.
- Keep code simple. Avoid over-engineering. If you find yourself adding a
  trait layer or a config system for hypothetical future needs, stop and ask.

### Frontend

```bash
cd frontend
npm install
npm run dev
```

The dev server proxies API calls to `http://localhost:8080` by default.

### Running Both

The repo's `start.sh` (backend) and `frontend/start-dev.sh` scripts start both
services in dev mode. PM2 configs in `ecosystem.config.cjs` handle longer-running
setups.

---

## Code Style

### Rust

- `cargo fmt` before committing (we may add a CI check).
- `cargo clippy` — address warnings or explain with `#[allow(...)]` + comment.
- Prefer `Result<T, Error>` over `unwrap()` / `expect()` outside tests.
- Public APIs should have rustdoc comments; private helpers generally should not.

### TypeScript / React

- ESLint config in `frontend/eslint.config.js`. Run `npm run lint`.
- Prefer function components and hooks.
- Keep components small and typed; avoid `any`.
- Tailwind utility classes are fine for styling; avoid inline `style={...}`
  except for dynamic values.

### General

- No commented-out code in merged PRs.
- No `console.log` / `dbg!()` / `TODO` without an issue reference.
- Respect project policy: simplicity over cleverness.

---

## What We Won't Merge (Quickly)

- Large refactors without prior discussion.
- New runtime dependencies without justification (each dependency is a
  maintenance and security cost).
- Changes that break the one-command dev bootstrap (`docker compose up` /
  `cargo build && ./start.sh`).
- Features that require closed-source components.

## What We Prioritize

- Security fixes.
- Correctness fixes with reproducible test cases.
- Documentation that unblocks new users.
- Orchard / Zcash protocol compliance work.
- Selective Disclosure API surface (view key, payment proof, audit range).
- Deployability improvements (Dockerfile, Helm, systemd units).

---

## Licensing

By contributing, you agree that your contributions will be licensed under the
[Apache License 2.0](LICENSE), and that you have the right to submit the work
under that license. If your employer has rights to intellectual property you
create, ensure you have permission to contribute on its behalf.

For significant contributions we may request a Developer Certificate of
Origin (DCO) sign-off on commits (`git commit -s`).

---

## Getting Help

- **General questions / design discussion**: GitHub Discussions
- **Bugs / feature requests**: GitHub Issues
- **Security**: `security@fastaitop.com` (see [SECURITY.md](SECURITY.md))
- **Real-time chat**: (link to Discord / forum thread — coming soon)

Thank you for contributing to zpay-enterprise. Privacy-preserving financial
infrastructure is built one careful PR at a time.
