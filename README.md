[English](README.md) | [中文](README_CN.md)

# Web3 Wallet Service

[![License: Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)
[![Contributions welcome](https://img.shields.io/badge/contributions-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Security policy](https://img.shields.io/badge/security-disclosure-red.svg)](SECURITY.md)

A modular Web3 wallet management service with multi-chain support, featuring a Rust backend and React frontend. **Now with full Zcash Orchard privacy protocol support.**

> 🚀 **5-Minute Quick Start (Docker):**
>
> ```bash
> git clone --branch v0.2.0 https://github.com/robustfengbin/zpay-enterprise.git
> cd zpay-enterprise
> cp backend/.env.example .env
> docker compose up --build
> ```
>
> Backend boots on `http://localhost:8080`. On first start, missing secrets
> (encryption key, JWT secret, admin password) are auto-generated and written
> to `backend/.env.secrets` — back up that file, **its loss = permanent loss
> of all encrypted wallets**.
>
> Full walkthrough: [QUICKSTART.md](QUICKSTART.md) · Recent changes: [CHANGELOG.md](CHANGELOG.md) · Latest release: **v0.2.0**

## Features

- **Wallet Management** - Create, import, and manage multiple wallets with encrypted private key storage
- **Multi-Chain Support** - Extensible architecture for multiple blockchain networks (Ethereum, Zcash)
- **Token Support** - Native tokens and ERC20 tokens (USDT, USDC, DAI, WETH)
- **Zcash Privacy** - Full Orchard protocol with Halo 2 zero-knowledge proofs
- **Four Transfer Modes** - Complete Zcash transfer types (T→T, T→Z, Z→Z, Z→T)
- **Transfer Management** - Initiate, execute, and track transactions with real-time status updates
- **Gas Estimation** - EIP-1559 compatible gas fee estimation
- **RPC Management** - Dynamic RPC endpoint configuration with fallback support
- **Role-Based Access** - Admin and Operator roles with permission controls
- **Internationalization** - Multi-language frontend support

---

## Screenshots

### Dashboard
![Dashboard](docs/images/dashboard.png)

### Zcash Wallet Management
Zcash wallet with unified addresses, transparent/shielded balance display, and Privacy Notes viewer.
![Zcash Wallet](docs/images/zcash_account.jpg)

### Ethereum Transfer
ERC20 token transfer with EIP-1559 gas estimation and balance preview.
![Ethereum Transfer](docs/images/transfer.jpg)

### Zcash Privacy Transfer (Z→Z)
Shielded-to-shielded transfer with encrypted memo and Halo 2 proof generation.
![Zcash Privacy Transfer](docs/images/prepare_transfer_z_z.jpg)

### Transfer Success
Transaction submitted successfully with transaction hash.
![Transfer Success](docs/images/transfer_tx.jpg)

### Transfer History
Complete transfer history with shielded transaction tracking.
![Transfer History](docs/images/transfer_his.jpg)

### RPC Node Settings
Multi-provider RPC configuration with Alchemy, Infura, QuickNode support.
![RPC Settings](docs/images/node_rpc.jpg)

---

## Product Vision & Roadmap

We are building the **Enterprise-Grade Privacy Finance Infrastructure for Web3** — the world's first platform that enables companies to move money on public blockchains with the same privacy, security, and control as traditional financial systems.

### What We Are Building

> **Privacy-First Financial Operating System for Web3**
>
> *"Stripe + Treasury + Privacy Layer for Crypto"*

A unified platform where enterprises can:
- Accept, store, move, and settle digital assets
- Use Zcash shielded pools for privacy
- Use public chains for liquidity
- Control who can move money and under what conditions
- Generate compliance-ready audit trails

### 2026 Roadmap

| Quarter | Focus | Key Deliverables |
|---------|-------|------------------|
| **Q1** | Enterprise Reliability | End-to-end transaction tracking, auto failover, real-time dashboards |
| **Q2** | Compliance & Governance | Multi-user approval workflows, audit trails, enterprise webhooks |
| **Q3** | High-Volume Privacy | Optimized Orchard sync, large-value transfers, unified balance management |
| **Q4** | Privacy Finance Platform | Developer SDKs, multi-chain treasury, HSM/KMS integrations |

### Long-Term Vision

By 2026, we will power:
- Crypto exchanges protecting user deposits
- OTC desks settling billion-dollar trades privately
- Payment processors offering privacy by default
- Web3 companies running confidential payroll and treasury

> 📄 **[Read Full Roadmap (English)](docs/product-roadmap-2026.md)** | **[中文版](docs/product-roadmap-2026-cn.md)**

---

## Zcash Privacy Transfer Modes

The system implements **all four Zcash transfer modes**, providing complete flexibility for privacy management:

### Transfer Mode Comparison

| Mode | From | To | Privacy Level | Use Case |
|------|------|-----|---------------|----------|
| **T→T** | Transparent | Transparent | None | Standard public transactions |
| **T→Z** | Transparent | Shielded | Partial | Shielding funds for privacy |
| **Z→Z** | Shielded | Shielded | Maximum | Fully private transactions |
| **Z→T** | Shielded | Transparent | Partial | Deshielding for exchanges |

### Mode Details

#### 1. Transparent to Transparent (T→T)

```
┌─────────────┐                    ┌─────────────┐
│  t1abc...   │ ───── ZEC ──────▶  │  t1xyz...   │
│ (Sender)    │                    │ (Receiver)  │
└─────────────┘                    └─────────────┘
         Public on blockchain
```

- **Privacy**: None - all details visible on blockchain
- **Speed**: Fast (~75 seconds confirmation)
- **Use Case**: Public payments, exchange deposits/withdrawals
- **API**: `POST /api/v1/transfers`

#### 2. Transparent to Shielded (T→Z Shielding)

```
┌─────────────┐                    ┌─────────────┐
│  t1abc...   │ ───── ZEC ──────▶  │  u1xyz...   │
│ Transparent │      Shielding     │  Shielded   │
└─────────────┘                    └─────────────┘
    Visible                          Hidden
```

- **Privacy**: Partial - sender visible, receiver hidden
- **Proof**: Halo 2 zero-knowledge proof generated
- **Use Case**: Moving funds into privacy pool
- **API**: `POST /api/v1/transfers/orchard` with `fund_source: "Transparent"`

#### 3. Shielded to Shielded (Z→Z)

```
┌─────────────┐                    ┌─────────────┐
│  u1abc...   │ ───── ZEC ──────▶  │  u1xyz...   │
│  Shielded   │   Full Privacy     │  Shielded   │
└─────────────┘                    └─────────────┘
    Hidden          Hidden            Hidden
         Maximum Privacy
```

- **Privacy**: Maximum - sender, receiver, and amount all hidden
- **Proof**: Full Halo 2 proof (spend + output)
- **Memo**: Optional 512-byte encrypted memo support
- **Use Case**: Private payments, confidential business transactions
- **API**: `POST /api/v1/transfers/orchard` with `fund_source: "Shielded"`

#### 4. Shielded to Transparent (Z→T Deshielding)

```
┌─────────────┐                    ┌─────────────┐
│  u1abc...   │ ───── ZEC ──────▶  │  t1xyz...   │
│  Shielded   │    Deshielding     │ Transparent │
└─────────────┘                    └─────────────┘
    Hidden                           Visible
```

- **Privacy**: Partial - sender hidden, receiver visible
- **Proof**: Halo 2 spend proof required
- **Use Case**: Exchange deposits, public payments from private funds
- **API**: `POST /api/v1/transfers/orchard` with transparent recipient address

### Technical Implementation

The Orchard privacy system uses:

- **Halo 2**: Recursive zero-knowledge proof system (no trusted setup)
- **Commitment Tree**: Merkle tree tracking all shielded notes
- **Nullifiers**: Prevent double-spending without revealing note identity
- **Incremental Witnesses**: Efficient proof path updates

```rust
// Fund source selection
pub enum FundSource {
    Auto,         // System chooses optimal source
    Shielded,     // Force use shielded funds (Z→Z or Z→T)
    Transparent,  // Force use transparent funds (T→Z)
}
```

### Fee Structure (ZIP-317)

| Actions | Fee (ZEC) |
|---------|-----------|
| 1-2 | 0.0001 |
| 3-4 | 0.00015 |
| 5+ | 0.00005 per additional |

---

## Enterprise Use Cases

This system is designed for enterprise-grade cryptocurrency management with privacy features:

### 1. Cryptocurrency Payment Gateway

- **Scenario**: E-commerce platforms accepting ZEC payments
- **Features Used**:
  - Multi-wallet management for different merchants
  - T→Z shielding for customer privacy
  - Real-time balance and transaction tracking
  - Webhook notifications for payment confirmation

### 2. Treasury Management System

- **Scenario**: Corporate treasury holding and managing crypto assets
- **Features Used**:
  - Role-based access (Admin/Operator separation of duties)
  - Audit logs for compliance
  - Multi-signature workflow (initiate → approve → execute)
  - Encrypted private key storage with HSM integration potential

### 3. OTC Trading Desk

- **Scenario**: High-volume OTC cryptocurrency trading
- **Features Used**:
  - Z→Z transfers for confidential large trades
  - Privacy protection for trade counterparties
  - Batch transaction processing
  - RPC failover for reliability

### 4. Privacy-Focused Exchange

- **Scenario**: Exchange offering privacy coin support
- **Features Used**:
  - T→Z for customer deposit shielding
  - Z→T for withdrawal processing
  - Automated balance reconciliation
  - Compliance-ready audit trails

### 5. Cross-Border Payment Service

- **Scenario**: International remittance with privacy requirements
- **Features Used**:
  - Multi-chain support (ETH for speed, ZEC for privacy)
  - Unified address management
  - Transaction memo for payment references
  - Multi-language interface

### 6. Institutional Custody Solution

- **Scenario**: Custodian managing crypto for institutional clients
- **Features Used**:
  - Segregated wallet per client
  - View-only keys for auditors
  - Cold/hot wallet separation
  - Comprehensive logging and reporting

### 7. DeFi Protocol Backend

- **Scenario**: DeFi protocol requiring privacy features
- **Features Used**:
  - Programmable transaction workflows
  - Gas optimization for Ethereum operations
  - Privacy pool integration via Orchard
  - API-first architecture for integration

### Deployment Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Enterprise Deployment                        │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────┐    │
│  │   Frontend   │     │   Backend    │     │   Database   │    │
│  │   (React)    │────▶│   (Rust)     │────▶│   (MySQL)    │    │
│  │   Port 3000  │     │   Port 8080  │     │   Port 3306  │    │
│  └──────────────┘     └──────────────┘     └──────────────┘    │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │                    Blockchain Layer                       │  │
│  │  ┌─────────────┐              ┌─────────────┐            │  │
│  │  │  Ethereum   │              │   Zcash     │            │  │
│  │  │  RPC Node   │              │  RPC Node   │            │  │
│  │  │ (Geth/Infura)│             │(Zebrad/Zcashd)│           │  │
│  │  └─────────────┘              └─────────────┘            │  │
│  └──────────────────────────────────────────────────────────┘  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Security Considerations for Enterprise

| Aspect | Implementation |
|--------|----------------|
| Key Storage | AES-256-GCM encryption at rest |
| Authentication | JWT with configurable expiration |
| Authorization | Role-based (Admin/Operator) |
| Audit | Comprehensive audit logging |
| Network | HTTPS/TLS required in production |
| Secrets | Environment-based configuration |

---

## Tech Stack

### Backend
- **Rust** with Actix-web 4
- **MySQL 5.7+** with SQLx
- **Ethers-rs** for Ethereum integration
- **Orchard/Zcash crates** for Zcash privacy protocol
- **Halo 2** zero-knowledge proof system
- **AES-256-GCM** encryption for private keys
- **JWT** authentication

### Frontend
- **React 19** with TypeScript
- **Vite** build tool
- **Tailwind CSS**
- **i18next** for internationalization

## Project Structure

```
github_web3_wallet_service/
├── backend/                    # Rust backend service
│   ├── src/
│   │   ├── api/               # REST API endpoints and middleware
│   │   ├── blockchain/        # Chain clients and token definitions
│   │   │   ├── ethereum/      # Ethereum client and ERC20 tokens
│   │   │   └── zcash/         # Zcash client and Orchard protocol
│   │   │       └── orchard/   # Halo 2 proofs, notes, witnesses
│   │   ├── services/          # Business logic layer
│   │   ├── db/                # Database models and repositories
│   │   ├── crypto/            # Encryption and password hashing
│   │   └── config/            # Configuration management
│   └── Cargo.toml
│
└── frontend/                   # React TypeScript frontend
    ├── src/
    │   ├── pages/             # Page components
    │   ├── components/        # Reusable UI components
    │   ├── services/          # API client modules
    │   └── hooks/             # Custom React hooks
    └── package.json
```

## Quick Start

### Prerequisites

- Rust (latest stable)
- Node.js 18+
- MySQL 5.7+

### Backend Setup

1. Navigate to the backend directory:
```bash
cd backend
```

2. Copy the environment file and configure:
```bash
cp .env.example .env
```

3. Configure your `.env` file:
```env
# Server
WEB3_SERVER__HOST=127.0.0.1
WEB3_SERVER__PORT=8080

# Database
WEB3_DATABASE__HOST=localhost
WEB3_DATABASE__PORT=3306
WEB3_DATABASE__USER=root
WEB3_DATABASE__PASSWORD=your_password
WEB3_DATABASE__NAME=web3_wallet

# JWT
WEB3_JWT__SECRET=your-secure-jwt-secret-key
WEB3_JWT__EXPIRE_HOURS=24

# Security (must be exactly 32 characters)
WEB3_SECURITY__ENCRYPTION_KEY=uK7m2VxQ9nL3aT1aR8c26yH0uJ4bZ5wE

# Ethereum
WEB3_ETHEREUM__RPC_URL=https://eth.llamarpc.com
WEB3_ETHEREUM__CHAIN_ID=1
```

4. Start the backend:
```bash
# Development mode
./start.sh run

# Production mode
./start.sh run-release

# Using PM2
./start.sh pm2
```

### Frontend Setup

1. Navigate to the frontend directory:
```bash
cd frontend
```

2. Install dependencies:
```bash
npm install
```

3. Start the development server:
```bash
npm run dev
```

4. Build for production:
```bash
npm run build
```

### Initial Admin Account

zpay-enterprise requires three secrets at startup:

- `WEB3_SECURITY__ENCRYPTION_KEY` — 32-byte AES-256 key for encrypting wallet private keys
- `WEB3_JWT__SECRET` — HMAC secret for JWT session tokens
- `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD` — initial admin login (>=12 chars, not `admin123`)

You have two options:

**Option A — auto-generate (simplest, recommended for first-time users):**
leave all three variables unset (or empty) in `backend/.env`. On first
startup the service will generate strong random values, write them to
`backend/.env.secrets` (chmod 0600, gitignored), and print the file location.
Subsequent restarts reuse the same file — secrets are never silently rotated,
which would make existing encrypted wallets unrecoverable.

> ⚠️ **If you rely on auto-generation, back up `backend/.env.secrets`.**
> Loss of that file = permanent loss of all encrypted wallets.

**Option B — explicit values (recommended for production):**
set all three in your `.env` file, container runtime, or secrets manager.
The service validates them at startup and refuses to start on weak values.

After you log in the first time, change the admin password via the UI
(Settings → Change Password).

- **Username:** `admin`
- **Password:** either (A) the value written to `backend/.env.secrets`, or
  (B) the value you set in `WEB3_SECURITY__ADMIN_INITIAL_PASSWORD`

## API Reference

### Authentication
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/auth/login` | User login |
| POST | `/api/v1/auth/logout` | User logout |
| PUT | `/api/v1/auth/password` | Change password |
| GET | `/api/v1/auth/me` | Get current user info |

### Wallets
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/wallets` | List all wallets |
| POST | `/api/v1/wallets` | Create new wallet |
| POST | `/api/v1/wallets/import` | Import wallet from private key |
| GET | `/api/v1/wallets/{id}` | Get wallet details |
| DELETE | `/api/v1/wallets/{id}` | Delete wallet |
| PUT | `/api/v1/wallets/{id}/activate` | Set as active wallet |
| POST | `/api/v1/wallets/{id}/export-key` | Export private key |
| GET | `/api/v1/wallets/balance` | Get wallet balance |

### Transfers
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/transfers` | List transfers with pagination |
| POST | `/api/v1/transfers` | Initiate new transfer |
| GET | `/api/v1/transfers/{id}` | Get transfer details |
| POST | `/api/v1/transfers/{id}/execute` | Execute pending transfer |
| POST | `/api/v1/transfers/estimate-gas` | Estimate gas fees |

### Zcash Orchard (Privacy)
| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/v1/wallets/{id}/orchard/enable` | Enable Orchard for wallet |
| GET | `/api/v1/wallets/{id}/orchard/addresses` | Get unified addresses |
| GET | `/api/v1/wallets/{id}/orchard/balance` | Get shielded balance |
| GET | `/api/v1/wallets/{id}/orchard/balance/combined` | Get combined balance |
| GET | `/api/v1/wallets/{id}/orchard/notes` | List unspent notes |
| POST | `/api/v1/transfers/orchard` | Initiate privacy transfer |
| POST | `/api/v1/transfers/orchard/{id}/execute` | Execute privacy transfer |
| GET | `/api/v1/zcash/scan/status` | Get sync status |
| POST | `/api/v1/zcash/scan/sync` | Trigger manual sync |

### Settings
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/settings/rpc` | Get current RPC config |
| PUT | `/api/v1/settings/rpc` | Update RPC config |
| POST | `/api/v1/settings/rpc/test` | Test RPC endpoint |
| GET | `/api/v1/settings/rpc/presets` | Get RPC presets |

### Health
| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/v1/health` | Health check |

## Security

- **Private Key Encryption:** AES-256-GCM encryption for all private keys at rest
- **Password Hashing:** Argon2 algorithm for password security
- **JWT Authentication:** Stateless authentication with configurable expiration
- **Role-Based Access Control:** Admin and Operator roles with different permissions
- **Sensitive Operation Protection:** Password verification required for private key export

### Security Best Practices

1. Change the default admin password immediately
2. Use a strong, random 32-byte encryption key
3. Use a cryptographically secure JWT secret
4. Enable HTTPS in production
5. Configure proper database access controls

## Database

The service automatically creates the required tables on startup:

- `users` - User accounts and roles
- `wallets` - Wallet information with encrypted private keys
- `transfers` - Transaction history and status
- `audit_logs` - Security audit trail
- `settings` - Application configuration
- `orchard_sync_state` - Zcash blockchain sync progress per wallet
- `orchard_notes` - Shielded notes (unspent outputs) with witness data
- `orchard_tree_state` - Commitment tree state for proof generation

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `WEB3_SERVER__HOST` | Server bind address | 127.0.0.1 |
| `WEB3_SERVER__PORT` | Server port | 8080 |
| `WEB3_DATABASE__HOST` | MySQL host | localhost |
| `WEB3_DATABASE__PORT` | MySQL port | 3306 |
| `WEB3_DATABASE__USER` | MySQL user | root |
| `WEB3_DATABASE__PASSWORD` | MySQL password | - |
| `WEB3_DATABASE__NAME` | Database name | web3_wallet |
| `WEB3_JWT__SECRET` | JWT signing secret | - |
| `WEB3_JWT__EXPIRE_HOURS` | Token expiration | 24 |
| `WEB3_SECURITY__ENCRYPTION_KEY` | 32-byte encryption key | - |
| `WEB3_ETHEREUM__RPC_URL` | Ethereum RPC endpoint | - |
| `WEB3_ETHEREUM__CHAIN_ID` | Ethereum chain ID | 1 |
| `WEB3_ETHEREUM__RPC_PROXY` | Optional RPC proxy | - |
| `WEB3_ZCASH__RPC_URL` | Zcash RPC endpoint | - |
| `WEB3_ZCASH__RPC_USER` | Zcash RPC username | - |
| `WEB3_ZCASH__RPC_PASSWORD` | Zcash RPC password | - |
| `WEB3_ZCASH__RPC_PROXY` | Optional Zcash RPC proxy | - |

### Frontend Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `VITE_API_BASE_URL` | Backend API URL | http://localhost:8080/api/v1 |

## PM2 Deployment

The backend includes PM2 configuration for production deployment:

```bash
# Start with PM2
./start.sh pm2

# View status
./start.sh status

# Stop
./start.sh pm2-stop

# Restart
./start.sh pm2-restart
```

## Extending

### Adding New Chains

Implement the `ChainClient` trait in `backend/src/blockchain/traits.rs`:

```rust
#[async_trait]
pub trait ChainClient: Send + Sync {
    async fn get_balance(&self, address: &str) -> Result<String>;
    async fn get_token_balance(&self, address: &str, token: &str) -> Result<String>;
    async fn send_transaction(&self, tx: TransactionRequest) -> Result<String>;
    // ... other methods
}
```

### Adding New Tokens

Add token definitions in `backend/src/blockchain/ethereum/tokens.rs`:

```rust
pub static SUPPORTED_TOKENS: Lazy<HashMap<&'static str, TokenInfo>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert("NEW_TOKEN", TokenInfo {
        address: "0x...",
        decimals: 18,
        symbol: "NEW",
    });
    m
});
```

## Logging

Backend logs are written to `backend/logs/web3-wallet.log` with:
- 500MB file size limit
- 10 backup files rotation
- Configurable log level via `RUST_LOG`

```bash
# Example log configuration
RUST_LOG=info,sqlx=warn
```

## Support

If you find this project useful, consider supporting the development:

**ETH / USDT / USDC (ERC20):** `0xD76f061DaEcfC3ddaD7902A8Ff7c47FC68b3Dc49`

## License

Released under the [Apache License 2.0](LICENSE).

## Security

Found a vulnerability? Please report it privately — see [SECURITY.md](SECURITY.md).
Do not open a public issue for security reports.

## Contributing

Bug reports, feature ideas, and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, and
submission guidelines.

Quick version:

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## Acknowledgements

Built on [Zcash Orchard](https://github.com/zcash/orchard) + [Halo 2](https://github.com/zcash/halo2),
[Ethers-rs](https://github.com/gakonst/ethers-rs), and [Actix Web](https://actix.rs).
