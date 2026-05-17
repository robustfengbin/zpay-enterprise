# Staging deployment SOP

Bring a fresh staging host from clean OS to a live `feat/m1-2026-06`
deployment in ~30 minutes. Built for a 4-vCPU / 8 GB / 100 GB Linux
host with Docker available; smaller works for non-Zcash-RPC use cases.

For local dev, see [QUICKSTART.md](../QUICKSTART.md). For prod
hardening, also read [SECURITY.md](../SECURITY.md) — the items below
are the deltas specific to staging.

---

## 1. Prerequisites

| Component | Version | Purpose |
|---|---|---|
| Linux | Ubuntu 24.04 / Debian 12 (tested) | Host OS |
| Docker Engine | 20+ with Compose v2 | MySQL + Zebra |
| Rust toolchain | stable, latest | Backend build (`cargo build`, **debug** — never `--release` per CLAUDE.md C-1) |
| Node + Yarn | Node 20+, Yarn 1.22+ | Frontend build |
| `libssl-dev`, `pkg-config` | latest | Backend reqwest TLS link |
| nginx | 1.24+ | Reverse proxy + static host |
| pm2 | 5.x | Backend supervisor |
| `jq`, `curl` | latest | Smoke harness |

```bash
sudo apt update
sudo apt install -y libssl-dev pkg-config nginx jq curl build-essential
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker $USER && newgrp docker
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt install -y nodejs && sudo npm i -g yarn pm2
```

---

## 2. Clone + branches

```bash
git clone https://github.com/<your-org>/zpay-enterprise.git
cd zpay-enterprise
git checkout feat/m1-2026-06
```

The branch tip should match the latest M1.W2 commit (see
`memory/project_zpay_enterprise_m1.md` in the team workspace).

---

## 3. Zebra mainnet RPC (Docker)

The backend resolves disclosure range timestamps via Zebra's
`getblock` RPC, so a running mainnet node is required. Initial sync
takes ~12 hours on a fast disk.

```bash
docker run -d --name zebra-mainnet \
  --restart unless-stopped \
  -p 127.0.0.1:8232:8232 \
  -p 8233:8233 \
  -v /var/lib/zebra:/home/zebra/.cache/zebra \
  zfnd/zebra:latest
```

Verify cookie auth works:

```bash
COOKIE=$(docker exec zebra-mainnet cat /home/zebra/.cache/zebra/.cookie | cut -d: -f2)
curl -s -u "__cookie__:$COOKIE" -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"1.0","id":"1","method":"getblockchaininfo"}' \
  http://127.0.0.1:8232 | jq '.result.blocks'
```

You should see a block height climbing toward the chain tip. Wait for
`verificationprogress == 1.0` before running disclosure-range smoke.

**Cookie rotates on container restart** — see §6 for how the backend
re-reads it.

---

## 4. MySQL (Docker)

```bash
docker run -d --name zpay-mysql \
  --restart unless-stopped \
  -e MYSQL_ROOT_PASSWORD="$(openssl rand -hex 24)" \
  -e MYSQL_DATABASE=web3_wallet \
  -p 127.0.0.1:3306:3306 \
  -v /var/lib/zpay-mysql:/var/lib/mysql \
  mysql:8.0
```

Grab the generated root password from `docker logs zpay-mysql 2>&1 | grep GENERATED`
or set it explicitly above. The backend auto-creates / migrates schemas
on startup (CLAUDE.md C-2), so no manual `mysql` step is needed.

---

## 5. Backend build

```bash
cd backend
cargo build              # debug — never --release
```

First build is ~3 minutes (deps fetch + compile); incremental ~5 s.

---

## 6. Backend `.env`

```bash
cp .env.example .env
chmod 600 .env
```

Edit the values that matter for staging:

```bash
# Required
WEB3_SERVER__HOST=127.0.0.1
WEB3_SERVER__PORT=8080
WEB3_SERVER__ALLOWED_ORIGIN=https://staging.zpay.example.com  # exact origin, no wildcards

WEB3_DATABASE__HOST=127.0.0.1
WEB3_DATABASE__PORT=3306
WEB3_DATABASE__USER=root
WEB3_DATABASE__PASSWORD=<the mysql root pw from §4>
WEB3_DATABASE__NAME=web3_wallet

# Leave the three secrets empty on first start — the backend auto-generates
# them and persists to backend/.env.secrets (chmod 0600, gitignored).
WEB3_JWT__SECRET=
WEB3_SECURITY__ENCRYPTION_KEY=
WEB3_SECURITY__ADMIN_INITIAL_PASSWORD=

# Zcash RPC via Zebra cookie auth — IMPORTANT: re-run this snippet after
# every `docker restart zebra-mainnet`; cookie rotates on restart.
WEB3_ZCASH__RPC_URL=http://127.0.0.1:8232
```

Append the rotating cookie auth pair without committing the value:

```bash
echo "WEB3_ZCASH__RPC_USER=__cookie__" >> .env
echo "WEB3_ZCASH__RPC_PASSWORD=$(docker exec zebra-mainnet \
  cat /home/zebra/.cache/zebra/.cookie | cut -d: -f2)" >> .env
```

---

## 7. Start backend under pm2

```bash
cd /path/to/zpay-enterprise/backend
pm2 start ./target/debug/web3_wallet_service \
  --name zpay-backend \
  --update-env

pm2 save                 # survive reboot via `pm2 startup`
pm2 logs zpay-backend --lines 30
```

You should see, in order: migrations OK lines, "Orchard proving key
ready", and `Starting HTTP server at 127.0.0.1:8080`. On first start
the auto-generated admin password is written to `backend/.env.secrets`
— record it in your secrets manager and `chmod 600` confirms it's
operator-readable only.

---

## 8. Frontend build + serve

```bash
cd /path/to/zpay-enterprise/frontend
yarn install --frozen-lockfile
VITE_API_BASE_URL=https://staging.zpay.example.com/api/v1 yarn build
```

The `dist/` directory is the static artifact. Copy / symlink it into
the nginx web root:

```bash
sudo mkdir -p /var/www/zpay-staging
sudo rsync -a --delete dist/ /var/www/zpay-staging/
```

---

## 9. nginx reverse proxy + SPA fallback

`/etc/nginx/sites-available/zpay-staging`:

```nginx
server {
    listen 443 ssl http2;
    server_name staging.zpay.example.com;

    ssl_certificate     /etc/letsencrypt/live/staging.zpay.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/staging.zpay.example.com/privkey.pem;

    root /var/www/zpay-staging;
    index index.html;

    # IMPORTANT: SPA cache rules — index.html must never be cached, hashed
    # assets must be cached aggressively.  See feedback_nginx_spa_cache_control
    # in the team workspace memory for the reasoning.
    #
    # Quirk: nginx's `expires` directive ALWAYS emits a Cache-Control: max-age
    # header that overrides anything from `add_header Cache-Control`.  Use
    # `add_header ... always;` (the `always` modifier is what lets the header
    # apply on 304 / 4xx responses) and DO NOT add `expires` here — otherwise
    # the response will surface `Cache-Control: max-age=0` instead of the
    # strict no-store policy we want.  Verified live 2026-05-17 on staging.
    location = /index.html {
        add_header Cache-Control "no-cache, no-store, must-revalidate" always;
    }
    location /assets/ {
        expires 1y;
        add_header Cache-Control "public, immutable";
    }

    # API reverse proxy.  Forward X-Forwarded-For so the backend's per-IP
    # login rate limiter (actix-governor) sees the real client.
    location /api/ {
        proxy_pass         http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header   Host              $host;
        proxy_set_header   X-Real-IP         $remote_addr;
        proxy_set_header   X-Forwarded-For   $proxy_add_x_forwarded_for;
        proxy_set_header   X-Forwarded-Proto $scheme;
        proxy_read_timeout 120s;             # disclosure async polls can run long
    }

    # SPA fallback.  Must come last; the API + asset locations above win.
    location / {
        try_files $uri $uri/ /index.html;
    }
}

server {
    listen 80;
    server_name staging.zpay.example.com;
    return 301 https://$host$request_uri;
}
```

```bash
sudo ln -s /etc/nginx/sites-available/zpay-staging /etc/nginx/sites-enabled/
sudo certbot --nginx -d staging.zpay.example.com
sudo nginx -t && sudo systemctl reload nginx
```

---

## 10. Verify

```bash
curl -fsS https://staging.zpay.example.com/api/v1/health | jq
# {"status":"ok","version":"..."}
```

Then run the full e2e harness (writes to the staging DB — only run
once or after a `--reset` cycle):

```bash
cd /path/to/zpay-enterprise
./e2e/smoke.sh
# expected: 34/34 PASS
```

If a disclosure range step fails with a Zcash RPC error, the cookie
has almost certainly rotated — re-run the snippet from §6 and
`pm2 restart zpay-backend --update-env`.

---

## 11. Operations

### Rollback

```bash
cd /path/to/zpay-enterprise
git fetch origin
git checkout <previous-known-good-commit>
cd backend && cargo build && pm2 restart zpay-backend --update-env
cd ../frontend && yarn build && sudo rsync -a --delete dist/ /var/www/zpay-staging/
```

The backend's auto-migrate is forward-only by design (CLAUDE.md C-2);
DB rollback past a migration requires a manual restore from MySQL
dump. Take a dump before any deploy that bumps migration count:

```bash
docker exec zpay-mysql mysqldump -uroot -p"$DB_ROOT_PASSWORD" web3_wallet \
  > /var/backups/zpay-$(date +%F-%H%M).sql
```

### Logs

- Backend: `pm2 logs zpay-backend`, plus rotated files under
  `backend/logs/web3-wallet.log*` (10 × 500 MB rolling).
- Zebra: `docker logs -f zebra-mainnet`.
- MySQL: `docker logs zpay-mysql`.
- nginx: `/var/log/nginx/{access,error}.log`.

### Common gotchas

| Symptom | Cause | Fix |
|---|---|---|
| `401 / connection close` from Zcash RPC on disclosure range | Zebra cookie rotated | §6 snippet + `pm2 restart … --update-env` |
| `cargo build` fails on `openssl-sys` | `libssl-dev` missing | `sudo apt install libssl-dev pkg-config` |
| Auditor pages return 404 | Frontend built against wrong `VITE_API_BASE_URL` | rebuild + rsync |
| New backend code not running | `pm2 restart` not after `cargo build` | confirm `ls -la target/debug/web3_wallet_service` timestamp |
| `.env` change not visible | `pm2 restart` without `--update-env` | always pass `--update-env` |
| Browser pulls stale JS after deploy | `index.html` was cached | nginx config in §9 sets `no-cache` for `index.html`; flush if missing |

---

## 12. What this SOP does NOT cover

- Production hardening (CSP refinements, WAF, dedicated MySQL host,
  backup automation, monitoring SLOs).
- Container orchestration (kubernetes, helm). Staging runs single-host
  pm2 + docker on purpose — keeps the operator surface tiny.
- High-availability Zebra (single node is fine for staging; prod
  benefits from a hot standby).
- Auto-cookie wrapper for Zebra. Manual re-keying is the M1 path; the
  wrapper is M2 deployment polish.
