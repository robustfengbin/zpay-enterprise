# PRD-F1.1 — Viewing Key 审计层 + ZIP-307 Payment Disclosure

> **状态**：Draft v0.1 · 2026-05-16
> **分支**：`feat/m1-2026-06`
> **属于**：M1 (2026-06) P0 主线 1/3 · 合规维度 (D1)
> **主笔**：france 🥖（§1-5/§7-8）· sweden 🇸🇪（§6 Frontend spec）
> **关联**：`docs/feature-backlog-2026.md` §3.1 F1.1

> **一句话**：让雇主一键导出 Orchard viewing key 给审计师 + 生成 ZIP-307 payment disclosure 报告（PDF/CSV），实现 **Zcash 协议原生的事后合规可审计**，比 Privacy Pools ASP 更优雅。

---

## §1 现状

- librustzcash 0.27 / Orchard 0.13 已集成（见 `backend/Cargo.toml`）
- `backend/src/blockchain/zcash/orchard/` Halo 2 proof + 4 转账模式齐全
- `audit_logs` 表已存在，**但仅平台操作审计，非链上 disclosure**
- RBAC 仅 `Admin` / `Operator` 二元（`backend/src/db/models.rs` `UserRole` enum）
- **无 viewing key 暴露 API、无 ZIP-307 disclosure 生成、无 Auditor 角色**
- 现有 Orchard `OrchardKeys` 结构（`backend/src/blockchain/zcash/orchard/keys.rs` 需扫）含 viewing key 派生能力但未暴露到 service / handler 层

> ⚠️ **librustzcash payment disclosure (ZIP-307) API 颗粒度需 verify** — W1 开工前 sub-agent 实测：是否支持「单笔 tx / 单收款地址 / 时间范围」三档；如果某档不支持，调整 §2 FR 与 §5 API。

---

## §2 需求

### 2.1 用户故事

1. **雇主 (Admin)**：审计师 / 监管 / 税务来查时，**一键导出 viewing key** 给对方，无需暴露 spending key
2. **审计师 (Auditor)**：独立账户登录，看授权 wallet 的进出明细 + 余额 + 历史 disclosure，**不能动钱**、**不能改设置**
3. **监管**：收到 ZIP-307 payment disclosure PDF/CSV，能离线验证 + 重建该时间窗的资金流

### 2.2 功能性需求 (FR)

| ID | 需求 | 优先级 |
|---|---|---|
| F1.1.1 | Orchard **outgoing viewing key (OVK)** 导出 API | P0 |
| F1.1.2 | Orchard **incoming viewing key (IVK)** 导出 API | P0 |
| F1.1.3 | **Unified Full Viewing Key (UFVK)** 导出 API（含 t-addr + Orchard 全部 viewing 权限） | P0 |
| F1.1.4 | **Auditor 角色**（RBAC 新增第三种，跟 Admin/Operator 隔离登录） | P0 |
| F1.1.5 | Admin 邀请 Auditor + 绑定 wallet scope（单 wallet / 多 wallet）+ 时间窗限制 | P0 |
| F1.1.6 | Auditor **只读 dashboard**：授权 wallet 的 transfers / balance / 历史 disclosure | P0 |
| F1.1.7 | **ZIP-307 payment disclosure** 报告生成（异步任务） | P0 |
| F1.1.8 | 颗粒控制：单笔 tx / 单收款地址 / 时间范围 三档 | P0 |
| F1.1.9 | 报告导出：**PDF / CSV / JSON** 三格式 | P0 |
| F1.1.10 | viewing key 导出 / disclosure 生成 全部进 `audit_logs` | P0 |
| F1.1.11 | viewing key 一次性下载（服务端下载后清除明文，仅留 hash 记录） | P0 |

### 2.3 非功能性需求 (NFR)

| ID | 需求 |
|---|---|
| NFR-1 | viewing key **加密存储**（复用 AES-256-GCM + per-tenant 主密钥） |
| NFR-2 | Auditor 越权 **DB 层强制 scope check**（中间件 + repository 双重） |
| NFR-3 | disclosure 报告生成 **异步**（大时间窗可能 30s+），polling 进度 |
| NFR-4 | 报告文件 **TTL 7 天自动清除**（下载后或超期） |
| NFR-5 | 复用现有 i18n 框架，所有 UI 文案中英文 |

### 2.4 OOS (Out of Scope, M1)

- ❌ 邮件 / 短信验证审计师身份（M2 加 OTP）
- ❌ 多审计师协同（M3 加）
- ❌ 审计师导出原始 raw note 数据（仅汇总报告）
- ❌ 链上 zk-disclosure proof（M5+ 配合 ZIP-307 v2）

---

## §3 技术方案

### 3.1 viewing key 导出

```
[Admin POST /viewing-keys/export]
        ↓
WalletService.export_viewing_key(wallet_id, key_type, password)
        ↓
OrchardKeys::derive_viewing_keys(spending_key)
        ↓
serialize → AES-GCM 加密 → DB `viewing_key_exports` 表
        ↓
return one-time download token (TTL 24h, 单次)
```

- key_type ∈ {`ovk`, `ivk`, `ufvk`}
- 复用现有 `crypto/` 模块的 AES-256-GCM (`backend/src/crypto/`)
- 下载使用 random 32-byte token，**不走 JWT**（独立于登录态，可发给审计师）

### 3.2 ZIP-307 Payment Disclosure 生成

```
[Admin POST /wallets/{id}/payment-disclosure]
        ↓
PaymentDisclosureService.generate(wallet_id, granularity, scope_param, format)
        ↓
异步任务 (tokio::spawn) → 写 DB 状态 generating
        ↓
[librustzcash] 调用 Orchard payment disclosure builder
        ↓
按 granularity 过滤：tx_hash | recipient_addr | time_range
        ↓
组装 disclosure JSON → 渲染 PDF/CSV
        ↓
存 `payment_disclosures` 表 + 文件落 `./uploads/disclosures/`
        ↓
更新状态 ready → 推 webhook (M3 加)
```

- PDF 生成：`printpdf` crate（简单 HTML 模板，无复杂排版）
- CSV：标准 csv crate
- JSON：直接 serde_json::to_string_pretty

### 3.3 Auditor 角色实现

- `users` 表 `role` 字段已是 VARCHAR，新增枚举值 `Auditor`（兼容现有 Admin/Operator）
- 但 Auditor 数据 **物理隔离** → 新建 `auditors` 表（不复用 `users`，避免 Auditor 误入 Admin/Operator API）
- Auditor 走 `/api/v1/auditor/*` 独立路由前缀
- 独立 JWT secret（`WEB3_AUDITOR_JWT_SECRET`），隔离主用户登录态
- 中间件 `AuditorAuthMiddleware` 强制 scope check

### 3.4 复用现有代码点 inventory

| 现有 | 新增/复用方式 |
|---|---|
| `crypto/` AES-256-GCM | 直接复用加密 viewing key |
| `OrchardKeys` (`blockchain/zcash/orchard/keys.rs`) | 加 `derive_viewing_keys()` 方法 |
| `Wallet.encrypted_private_key` | 解密 → 派生 viewing key → 重新加密存 |
| `audit_logs` 表 | 写入 export/disclosure 操作记录 |
| `AuthMiddleware` | 复制为 `AuditorAuthMiddleware`，scope 校验逻辑加进去 |
| `WalletRepository` | 加 `find_by_id_with_auditor_scope(wallet_id, auditor_id)` |

---

## §4 数据模型

> 按 CLAUDE.md C-2，**所有表用 Rust + sqlx `CREATE TABLE IF NOT EXISTS` 启动时自动建**，不写 .sql 文件。

### 4.1 `auditors`

```sql
CREATE TABLE IF NOT EXISTS auditors (
  id INT AUTO_INCREMENT PRIMARY KEY,
  email VARCHAR(255) NOT NULL UNIQUE,
  password_hash VARCHAR(255) NOT NULL,
  name VARCHAR(255) NOT NULL,
  invited_by_user_id INT NOT NULL,
  active BOOLEAN NOT NULL DEFAULT TRUE,
  last_login_at DATETIME NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (invited_by_user_id) REFERENCES users(id),
  INDEX idx_email (email)
);
```

### 4.2 `auditor_wallet_scopes`

```sql
CREATE TABLE IF NOT EXISTS auditor_wallet_scopes (
  id INT AUTO_INCREMENT PRIMARY KEY,
  auditor_id INT NOT NULL,
  wallet_id INT NOT NULL,
  granted_by_user_id INT NOT NULL,
  scope_start_ts DATETIME NOT NULL,
  scope_end_ts DATETIME NOT NULL,
  max_disclosure_count INT NOT NULL DEFAULT 10,
  current_count INT NOT NULL DEFAULT 0,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (auditor_id) REFERENCES auditors(id),
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  UNIQUE KEY uniq_scope (auditor_id, wallet_id)
);
```

### 4.3 `viewing_key_exports`

```sql
CREATE TABLE IF NOT EXISTS viewing_key_exports (
  id INT AUTO_INCREMENT PRIMARY KEY,
  wallet_id INT NOT NULL,
  exported_by_user_id INT NOT NULL,
  key_type VARCHAR(16) NOT NULL,            -- ovk | ivk | ufvk
  encrypted_payload BLOB NOT NULL,          -- AES-GCM(viewing_key)
  payload_hash VARCHAR(64) NOT NULL,        -- SHA256(viewing_key) for audit
  download_token VARCHAR(64) NOT NULL UNIQUE,
  downloaded_at DATETIME NULL,
  downloaded_by_ip VARCHAR(64) NULL,
  expires_at DATETIME NOT NULL,             -- 24h from create
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (exported_by_user_id) REFERENCES users(id)
);
```

### 4.4 `payment_disclosures`

```sql
CREATE TABLE IF NOT EXISTS payment_disclosures (
  id INT AUTO_INCREMENT PRIMARY KEY,
  wallet_id INT NOT NULL,
  generated_by_user_id INT NOT NULL,
  granularity VARCHAR(16) NOT NULL,         -- tx | address | range
  scope_param JSON NOT NULL,                -- {tx_hash:.., addr:.., from:.., to:..}
  tx_count INT NOT NULL DEFAULT 0,
  disclosure_json JSON NULL,                -- ZIP-307 payload, NULL while generating
  format VARCHAR(16) NOT NULL,              -- pdf | csv | json
  file_path VARCHAR(512) NULL,
  status VARCHAR(16) NOT NULL DEFAULT 'generating',  -- generating | ready | failed
  error_message TEXT NULL,
  expires_at DATETIME NOT NULL,             -- 7 days TTL
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (generated_by_user_id) REFERENCES users(id),
  INDEX idx_wallet_status (wallet_id, status)
);
```

---

## §5 API 设计

### 5.1 Admin 侧 (走现有 `/api/v1/` 前缀 + `AuthMiddleware`)

| Method | Path | Body | Resp |
|---|---|---|---|
| POST | `/wallets/{id}/viewing-keys/export` | `{key_type, password}` | `{export_id, download_token, expires_at}` |
| GET | `/wallets/{id}/viewing-keys/exports` | — | `[ {id, key_type, downloaded_at, expires_at} ]` |
| POST | `/auditors` | `{email, name, wallet_ids[], scope_start, scope_end, max_count}` | `{auditor_id, invitation_link, temp_password}` |
| GET | `/auditors` | — | `[{id, email, name, scopes[], active}]` |
| PUT | `/auditors/{id}/deactivate` | — | `{ok}` |
| POST | `/wallets/{id}/payment-disclosures` | `{granularity, scope_param, format}` | `{disclosure_id, status:"generating"}` |
| GET | `/payment-disclosures/{id}` | — | `{id, status, tx_count, file_url?, error?}` |
| GET | `/payment-disclosures/{id}/download` | — | binary file (PDF/CSV/JSON) |
| GET | `/wallets/{id}/payment-disclosures` | — | `[{id, status, granularity, created_at}]` |

### 5.2 Auditor 侧 (`/api/v1/auditor/*` 独立前缀 + `AuditorAuthMiddleware`)

| Method | Path | Body | Resp |
|---|---|---|---|
| POST | `/auditor/login` | `{email, password}` | `{token, auditor_info}` |
| POST | `/auditor/password` | `{old, new}` | `{ok}` (强制首次登录改密) |
| GET | `/auditor/me` | — | `{id, email, name, scopes[]}` |
| GET | `/auditor/wallets` | — | `[{wallet_id, address, chain, scope_start, scope_end}]` |
| GET | `/auditor/wallets/{id}/balance` | — | `{native, tokens[]}` (走 viewing key 只读) |
| GET | `/auditor/wallets/{id}/transfers` | `?from=&to=&limit=&offset=` | `[{tx_hash, amount, ...}]` |
| GET | `/auditor/wallets/{id}/disclosures` | — | `[{id, granularity, created_at, file_url}]` |
| GET | `/auditor/disclosures/{id}/download` | — | binary file |

### 5.3 错误码

- `403 OUT_OF_SCOPE`：auditor 访问授权外 wallet / 时间窗外 transfer
- `429 DISCLOSURE_QUOTA_EXCEEDED`：超过 `max_disclosure_count`
- `410 EXPORT_EXPIRED`：viewing key 下载链接过期或已下载

---

## §6 Frontend Spec — sweden owner

<!-- ============================================================ -->
<!-- sweden:6 — Frontend spec 详化, 含路由 / 主要页面 / 组件列表 / -->
<!--           交互流程 / i18n key 命名 / 与 §5 API 的映射         -->
<!-- 建议结构：                                                    -->
<!-- §6.1 Admin 侧改造（Wallet 详情页加「合规审计」tab）             -->
<!-- §6.2 Auditor 独立登录 + dashboard 全新页面                     -->
<!-- §6.3 路由 + 状态管理（zustand？react-query？沿用现有）         -->
<!-- §6.4 一次性下载 UX（弹窗 + 警告 + 密码确认）                   -->
<!-- §6.5 异步生成进度 UX（toast + polling 或 SSE）                 -->
<!-- §6.6 i18n 中英文 key 列表                                      -->
<!-- ============================================================ -->

---

## §7 里程碑（4 周，2026-05-17 ~ 2026-06-13）

| 周 | 日期 | 后端 (france) | 前端 (sweden) | 验收 |
|---|---|---|---|---|
| **W1** | 5/17-5/23 | DB schema auto-migrate / OrchardKeys.derive_viewing_keys / `/viewing-keys/export` API + 加密下载 / Auditor 表 + 登录 / sub-agent 实测 ZIP-307 API 可行性 | 拆 wireframe / 提 i18n key 单 / 准备 audit tab 组件骨架 | curl 走通 export + download 闭环 |
| **W2** | 5/24-5/30 | ZIP-307 payment disclosure 生成（异步） / PDF/CSV/JSON 导出 / Auditor scope 中间件 + 只读 API | Wallet 详情页「合规审计」tab 实现 / 「导出 viewing key」弹窗 / 「邀请审计师」表单 | Postman 走完 6 个 Admin API + 3 个 Auditor API |
| **W3** | 5/31-6/6 | bug fix + perf / disclosure TTL 清理 cron / 越权测试 | Auditor 独立登录页 + dashboard / 报告下载 UX + 进度 polling / i18n 中英文 | e2e: 创建 auditor → 邀请链接登录 → 看 transfers → 生成报告 → 下载 |
| **W4** | 6/7-6/13 | bug list 一次性修 / 部署 dev / 准备 prod 切换 | 移动端响应式 / 极端 case 测试 / 文案 polish | smoke 8 个场景，prod 候选 |

---

## §8 风险登记

| ID | 风险 | 等级 | 缓解 |
|---|---|---|---|
| R1 | **librustzcash ZIP-307 payment disclosure API 颗粒度不够**（仅支持单笔 / 不支持时间范围） | 🔴 高 | W1 sub-agent 实测 librustzcash 0.27 disclosure API；如时间范围不支持，FR 改为单笔列表批量生成；最坏 fallback 自建 disclosure JSON 格式 |
| R2 | viewing key 一次性下载后泄露 | 🟠 中 | 服务端下载即删明文 + 仅留 hash + IP 记录 + audit_logs；24h TTL；触发"重新导出"必须重新登录 |
| R3 | Auditor 数据库越权（绕过中间件直查） | 🟠 中 | repository 层 `find_by_id_with_auditor_scope()` 强制注入 scope WHERE；写单元测试覆盖 6 种越权场景 |
| R4 | PDF 渲染性能（大时间窗 1000+ tx） | 🟡 低 | 异步生成 + 单次 max 1000 tx 限制 + 超额建议分批 disclosure |
| R5 | librustzcash 升级兼容性 | 🟡 低 | 沿用 Cargo.toml 0.27 全家族不动；新增依赖必须 `cargo tree` 确认无冲突 |
| R6 | 本机 zebrad 节点数据可用性 | 🟡 低 | Robust 5/16 confirm 本机节点有，sub-agent 在摸 config；fallback 公共 RPC |

---

## 附录 A · 编辑历史

- 2026-05-16 france — 初版 §1-§5 + §7-§8，§6 frontend 留 sweden 占位

## 附录 B · 术语表

- **OVK / IVK / UFVK** — Orchard outgoing / incoming / unified full viewing key
- **ZIP-307** — Payment Disclosure 提案，标准化 disclosure JSON 格式
- **Auditor** — 新 RBAC 第三种角色，独立于 Admin/Operator
- **Scope** — Auditor 可访问的 (wallet × 时间窗 × 配额) 三元组
