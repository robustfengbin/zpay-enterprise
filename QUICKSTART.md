# zpay-enterprise Quickstart

This guide gets you from a fresh clone to a running backend and a
successful API login in about five minutes, using Docker Compose.

> For background and full feature set, see [README.md](README.md).
> For security hardening before going live, see [SECURITY.md](SECURITY.md).

---

## 1. Prerequisites

- A 64-bit Linux or macOS machine (Oracle Cloud ARM tested, x86_64 works)
- [Docker Engine](https://docs.docker.com/engine/install/) 20+ **and**
  Docker Compose v2 (`docker compose version` should print `v2.x`)
- Ports `3306` and `8080` free on the host, **or** edit
  `docker-compose.yml` to remap them
- About 6 GB of free disk for the build cache and MySQL volume

You do **not** need Rust, Node.js, or MySQL installed on the host —
everything builds and runs inside containers.

---

## 2. Get the code

```bash
git clone https://github.com/robustfengbin/zpay-enterprise.git
cd zpay-enterprise
```

---

## 3. Configure secrets

Copy the example env file into a real `.env` at the repo root:

```bash
cp backend/.env.example .env
```

At a minimum, set these three values before first startup (the service
will refuse to start otherwise):

| Variable | Requirement | Example |
| --- | --- | --- |
| `WEB3_SECURITY__ENCRYPTION_KEY` | **exactly** 32 characters — used for AES-256-GCM of private keys | `openssl rand -hex 16` |
| `WEB3_JWT__SECRET` | any strong random string | `openssl rand -hex 32` |
| `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD` | at least 12 characters, cannot be `admin123` | `openssl rand -hex 12` |

### Option: let the service generate them for you

If you leave any of the three values empty, the backend will generate a
strong random value on first start, write it to `backend/.env.secrets`
(permissions `0600` where supported), and print a bright warning to
stderr. Back `.env.secrets` up — if you lose it, every encrypted wallet
key in the database becomes unreadable.

---

## 4. Bring the stack up

```bash
docker compose up --build -d
```

First build takes 3–6 minutes (it compiles the Rust backend in debug
mode; this is intentional per the project's `CLAUDE.md` rules).
Subsequent `docker compose up` starts in under 30 seconds.

You should see two healthy containers:

```bash
$ docker compose ps
NAME                        STATUS                    PORTS
zpay-enterprise-backend-1   Up                        0.0.0.0:8080->8080/tcp
zpay-enterprise-mysql-1     Up (healthy)              0.0.0.0:3306->3306/tcp
```

Tail the logs to confirm migrations ran and the HTTP server is up:

```bash
docker compose logs backend | tail -20
```

Look for:

```
Database migrations completed successfully
Default admin user created
Starting HTTP server at 0.0.0.0:8080
```

---

## 5. Log in via the API

Use the admin password you put in `.env` (or the one printed into
`backend/.env.secrets` if you let the service generate it):

```bash
curl -sS -X POST http://localhost:8080/api/v1/auth/login \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"YOUR_ADMIN_PASSWORD"}'
```

You should get back a JWT and the admin profile:

```json
{
  "token": "eyJ0eXAi...",
  "user": { "id": 1, "username": "admin", "role": "admin" }
}
```

Save the token in a shell variable:

```bash
TOKEN=$(curl -sS -X POST http://localhost:8080/api/v1/auth/login \
    -H "Content-Type: application/json" \
    -d '{"username":"admin","password":"YOUR_ADMIN_PASSWORD"}' \
    | python3 -c 'import sys,json;print(json.load(sys.stdin)["token"])')
```

All other endpoints expect `Authorization: Bearer $TOKEN`.

---

## 6. Create your first wallet

```bash
curl -sS -X POST http://localhost:8080/api/v1/wallets \
    -H "Authorization: Bearer $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"name":"treasury-1","chain":"ethereum"}'
```

The response includes the wallet id, the generated public address, and
the encrypted private key (encrypted with your `ENCRYPTION_KEY`).
The plaintext private key is never returned again.

List your wallets to confirm:

```bash
curl -sS http://localhost:8080/api/v1/wallets \
    -H "Authorization: Bearer $TOKEN" | python3 -m json.tool
```

---

## 7. Connect to a Zcash node (for shielded / Orchard operations)

Ethereum works out of the box against `https://eth.llamarpc.com` (a
public, rate-limited endpoint — fine for exploration, not for
production).

Zcash requires you to point at a node you control. Set
`WEB3_ZCASH__RPC_URL` in `.env` to one of:

- **Your own `zebrad` or `zcashd`** on the LAN. Run
  **Zebra ≥ 4.3.1** or **zcashd ≥ 6.12.1** — older versions have
  a consensus-split and DoS bug (see [SECURITY.md](SECURITY.md)).
- A `lightwalletd` endpoint you trust.

Without a reachable Zcash RPC, Ethereum operations still work; Orchard
shielded transfers will return an error until the node is configured.

---

## 8. Where the data lives

| What | Where |
| --- | --- |
| MySQL database | Docker volume `zpay-enterprise_mysql-data` |
| Backend logs (rotating, 500 MB × 10) | Docker volume `zpay-enterprise_backend-logs` |
| Auto-generated secrets | `backend/.env.secrets` on the **host** |
| Plaintext admin password (one-time, if printed) | `backend/ADMIN_PASSWORD.txt` on the host |

The two host-side files (`*.env.secrets`, `ADMIN_PASSWORD.txt`) are
listed in `.gitignore` — do not commit them.

---

## 9. Troubleshooting

**`address already in use` on port 8080 or 3306.**
Another process is using that port. Either stop it, or edit the
`ports:` section in `docker-compose.yml` to map to a different host
port (e.g. `"8090:8080"`).

**`WEB3_SECURITY__ADMIN_INITIAL_PASSWORD is too weak`.**
Set it to at least 12 characters and make sure it is not literally
`admin123`. The backend refuses to start until this is fixed — by
design, so fresh instances can't be compromised with default creds.

**`Encryption key must be exactly 32 bytes`.**
`WEB3_SECURITY__ENCRYPTION_KEY` must be **32 characters long**, not
32 bytes encoded as 64 hex chars. Use `openssl rand -hex 16` (which
gives a 32-character hex string).

**Backend container keeps restarting.**
```bash
docker compose logs backend | tail -40
```
is the first thing to look at — misconfigured env vars show up as
`Failed to load configuration` with a specific reason.

**Docker build fails on `core2 = "^0.3" is yanked`.**
You are building an old commit. Upgrade to `main` (or any commit from
2026-04-25 onwards): the Zcash stack was bumped to orchard 0.13 /
zcash_primitives 0.27, which moved off the yanked `core2`.

**`cargo build` locally fails with `edition2024 is required`.**
Your host toolchain is older than Rust 1.85.1. The Dockerfile uses
`rust:1.90-bookworm` which works. If you want to build outside of
Docker, install `rustup` and `rustup update stable` to pick up a
newer toolchain.

---

## 10. Production deployment (pointers, not a runbook)

This Quickstart brings the stack up for **evaluation**. Before taking
money:

- Move MySQL out of Docker Compose into a managed or hardened instance
  and drop the `mysql:` service from `docker-compose.yml`.
- Terminate TLS in front of port 8080 (nginx, Caddy, Traefik,
  Cloudflare Tunnel — any of them).
- Rotate the auto-generated secrets into your secret manager of choice;
  remove `backend/.env.secrets` from the host.
- Change the admin password via the UI after first login; consider
  removing the admin user entirely once a named operator account is
  created.
- Walk the [Hardening Checklist in SECURITY.md](SECURITY.md) end-to-end.
- Watch the backend process with your usual supervisor (systemd, PM2,
  Kubernetes liveness probes); the service exits non-zero on
  misconfiguration, so a supervisor will surface problems quickly.

---

## 11. Getting help

- **Bugs / feature requests:** open an issue on the GitHub repo.
- **Security vulnerabilities:** follow [SECURITY.md](SECURITY.md) —
  do **not** open a public issue.
- **Contributing:** see [CONTRIBUTING.md](CONTRIBUTING.md).
