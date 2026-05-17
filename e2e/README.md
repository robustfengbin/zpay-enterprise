# E2E smoke

One-shot happy-path coverage of M1 features (F1.1 + F2.1 + F3.1).
Runs 34 assertions across 11 steps.

## Usage

```bash
./e2e/smoke.sh              # against current DB + pm2 state (idempotent-ish — IDs roll)
./e2e/smoke.sh --reset      # drop+recreate web3_wallet DB, wipe .env.secrets, pm2 restart
```

Exit code: `0` on full pass, `1` on first failed assertion.

## What it covers

| # | Step | Notable assertions |
|---|---|---|
| 1 | Backend health | `GET /health` → `200 {status:"ok"}` |
| 2 | Admin login | Reads `.env.secrets` → `POST /auth/login` returns JWT |
| 3 | Wallet create | Eth + Zcash wallets; activate eth |
| 4 | F3.1 Employees CRUD | POST / GET / PUT (`active=false`) / DELETE |
| 5 | F2.1 ApprovalPolicy CRUD | `enabled` is bool (not i8), PUT + DELETE |
| 6 | F1.1 ViewingKey 3-step | export → download `orchard-ufvk:...` → re-download `403` |
| 7 | F1.1 Auditor full flow | create / login / `/me` / `/wallets` 11-field shape / `/balance` / `/transfers` / admin token rejected from `/auditor/*` |
| 8 | F3.1 PayrollRun | create + 422 invalid + `{run,items}` detail + execute tagged union |
| 9 | F2.1 auto-pivot | create policy threshold 0.0001 → run total 0.001 → `execute` returns `result=awaiting_approval` → `cancel` |
| 10 | F1.1 Disclosure async | POST `/payment-disclosures` → 202 `{status:"generating"}` → poll → `ready` → download returns `zip_version:"307-enterprise"` body with `actions[]`. Also asserts 400 on scope_param/granularity mismatch. |

## Env overrides

| var | default | purpose |
|---|---|---|
| `E2E_API` | `http://127.0.0.1:8080/api/v1` | base URL |
| `E2E_DB_CONTAINER` | `zhiling-admin-mysql` | docker container with MySQL |
| `E2E_DB_USER` / `E2E_DB_PASS` | `root` / `rootpw_change_me` | DB creds |
| `E2E_DB_NAME` | `web3_wallet` | target DB |
| `E2E_PM2_APP` | `web3-wallet-backend` | pm2 app name |
| `E2E_BACKEND_DIR` | `<repo>/backend` | for `.env.secrets` + `ecosystem.config.cjs` |

## Adding new flows

Append a `step "..."` block at the end of `smoke.sh`. Use `api METHOD path token body` + `parse_api` + `assert_pass` / `assert_fail`. Reference existing steps for the pattern.
