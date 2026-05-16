# PRD-F2.1 — Maker / Checker 双签转账

> **状态**：Draft v0.1 · 2026-05-16
> **分支**：`feat/m1-2026-06`
> **属于**：M1 (2026-06) P0 主线 2/3 · 治理维度 (D2)
> **主笔**：sweden 🇸🇪（§1-5/§7-8）· france 🥖（§6 Frontend spec 仅作技术验证 review，sweden 实现）
> **关联**：`docs/feature-backlog-2026.md` §3.2 F2.1

> **一句话**：把转账拆成「**发起（maker） → 审批（checker） → 执行（executor）**」三段，让企业财务/CFO/合规第一次愿意把真实业务资金交给本平台，同时满足 Q2 roadmap「资金治理」叙事。

---

## §1 现状

- 当前 RBAC 二元：`Admin` / `Operator`（`backend/src/db/models.rs` `UserRole` enum），无 Auditor / Maker / Checker 概念
- `Transfer` 状态机 4 态（`backend/src/db/models.rs` lines 87-95）：

  ```
  Pending → Submitted → Confirmed | Failed
  ```

- 转账流程 2 段：`POST /api/v1/transfers`（创建 Pending 记录）→ `POST /api/v1/transfers/{id}/execute`（链上提交）
- **任何登录用户（含 Operator）都可以独立完成创建 + 执行**，无审批门槛
- 没有「资金限额」「多人审批」概念，也无审批 SLA / 超时机制
- `audit_logs` 表能记录操作但**不能阻止**单人完成大额转账
- 同样问题在 Orchard 路径（`POST /transfers/orchard` + `/execute`）也存在

> ⚠️ 这条是 Q2 roadmap "合规与资金治理层" 的内核 — 企业财务/风控/审计部门第一次愿意接入隐私币的**前置条件**。

---

## §2 需求

### 2.1 用户故事

1. **Operator（Maker）**：我发起一笔转账，但平台告诉我「金额 ≥ \$5k 需要 Admin 审批」，转账进入 **AwaitingApproval** 状态，我能在「我的转账」里看到「待审批」标记，**审批员收到通知后做决定**
2. **Admin（Checker）**：我登录后看到「待我审批」列表，每条含金额 / 收款方 / 发起人 / 用途 memo / 风险标签，**一键批准或拒绝**（含拒绝原因）
3. **Operator（Executor）**：审批通过后，我（或自动机制）触发执行；审批未通过 / 已超时则不能执行
4. **Admin（合规人员）**：我能设置审批策略 — 「per-wallet 金额阈值」「per-token 阈值」「审批 SLA」「需要审批人数（M-of-N，v1 为 1-of-N）」
5. **Auditor（read-only）**：我能看整个审批历史 + 时间线，但不能动钱也不能改策略
6. **任意角色**：超过 SLA（默认 24h）未审批的转账自动 expire，maker 重新发起；audit_logs 完整记录

### 2.2 功能性需求（FR）

| ID | 需求 | 优先级 |
|---|---|---|
| FR-1 | `Transfer` 状态机扩展：增加 `AwaitingApproval` / `Approved` / `Rejected` / `Expired` 4 个新状态 | P0 |
| FR-2 | 新增 `transfer_approvals` 表记录每次审批决定（含 approver / decision / reason / 时间戳） | P0 |
| FR-3 | 新增 `approval_policies` 表，支持 scope = `global / wallet / user`，按 `chain + token + amount_threshold` 三键匹配 | P0 |
| FR-4 | `POST /transfers`：若 amount ≥ 适用策略阈值 → 状态直接 `AwaitingApproval` 而非 `Pending`，不能直接执行 | P0 |
| FR-5 | 新 `POST /transfers/{id}/approve` 接口：Admin 角色，**maker ≠ checker** 自校验 | P0 |
| FR-6 | 新 `POST /transfers/{id}/reject` 接口：必须带 `reason`（≥ 5 字符）| P0 |
| FR-7 | 修改 `POST /transfers/{id}/execute` 鉴权：状态必须为 `Pending`（≤ 阈值）或 `Approved`（≥ 阈值） | P0 |
| FR-8 | 新 `GET /transfers/pending-approval` 接口：返回当前用户**可审批**的转账列表（排除自己 maker 的） | P0 |
| FR-9 | 新 `GET/PUT /policies` 接口：Admin 配置审批策略；返回当前生效策略快照 | P0 |
| FR-10 | 后台 worker：扫描 `AwaitingApproval` 超过 SLA 的转账，自动改 `Expired` + 写 audit_logs | P0 |
| FR-11 | `audit_logs` 必须记录：create_pending_approval / approve / reject / expire / execute_with_approval 5 个事件 | P0 |
| FR-12 | Orchard 路径同步支持（`POST /transfers/orchard` / `/execute` 走同套审批） | P0 |
| FR-13 | Maker 可主动撤回未审批转账（`DELETE /transfers/{id}` 仅 `AwaitingApproval` 状态可用） | P1 |
| FR-14 | Email 通知（M1 quick win，复用 F6.2 渠道，先 stub）：审批请求 / 决定结果 | P1 |
| FR-15 | 大额二次密码确认（≥ \$25k 在 approve 时再验 password / TOTP）— P1，与 F2.4 合并实现 | P1 |

### 2.3 非功能性需求（NFR）

| ID | 需求 |
|---|---|
| NFR-1 | **状态机原子性**：approve 与 status 切换走 DB 事务，禁止两个 checker 同时 approve 产生重复审批 |
| NFR-2 | **向后兼容**：现有客户端 `POST /transfers/{id}/execute` 在「无策略匹配」「金额低于阈值」时仍直接 Submitted，不破坏现有自动化脚本 |
| NFR-3 | **审计完整性**：所有状态切换写 `audit_logs.details` JSON，含 from_status / to_status / actor_user_id / reason |
| NFR-4 | **幂等性**：approve / reject 接口支持 `Idempotency-Key` header，避免网络重试产生重复决定 |
| NFR-5 | **观测性**：Prometheus 指标 — pending_approval 队列长度 / 平均审批时长 / SLA 超时率（接 F4.4 告警） |
| NFR-6 | **MVP 仅 1-of-N 审批**：N-of-M 多审批级（F2.6）留到 M4，但表结构提前支持 `required_count` 字段（默认 1） |
| NFR-7 | **maker ≠ checker 硬约束**：DB unique 索引 (transfer_id, approver_user_id) + 应用层检查 transfer.created_by ≠ approver.id |
| NFR-8 | **CLAUDE.md C-2**：所有新表建表 SQL 走 `backend/src/db/migrations.rs` 启动时自动执行 |

### 2.4 范围之外（Out of Scope）

- ❌ N-of-M 多级审批（F2.6 留到 M4）
- ❌ 链上 multi-sig（与平台层审批不同，留到 F6.6 HSM 集成时一起规划）
- ❌ 移动端推送（F6.9 / F3.7 留到 M3-M5）
- ❌ 自动化审批规则（白名单收款方自动通过）— 留到 v2，本期所有人工
- ❌ 与外部审批系统对接（DocuSign / 飞书审批）— 留到 F6.7 ERP 适配器范畴

---

## §3 技术方案

### 3.1 状态机

```
                          (low amount, no policy)
                          ┌─────────────────────┐
                          │                     ▼
   [maker creates] ──→ Pending ─── /execute ──→ Submitted ──→ Confirmed
                          │                                    │
                          │ (high amount,                      ▼
                          │  policy matched)                 Failed
                          ▼
                   AwaitingApproval ──── /approve ──→ Approved ──┐
                          │                                       │
                          │ /reject                               │ /execute
                          ▼                                       ▼
                       Rejected                                Submitted ──→ Confirmed | Failed
                          ▲
                          │ SLA timeout
                  (after expiry_at)
                          │
                       Expired

   /transfers DELETE only allowed in AwaitingApproval (maker self-recall)
```

### 3.2 设计原则

1. **审批与执行分离**：`Approved` 是一个独立状态而非「approve 即 submit」，让 checker / executor 可以是不同人，也允许审批后等 gas 窗口再执行
2. **maker ≠ checker 是硬约束**：发起人不能审批自己的转账（DB 层 + 应用层双检）
3. **策略匹配最具体优先**：`user 策略 > wallet 策略 > global 策略`；同级别 token 精确匹配优先 default
4. **SLA 默认 24h**，per-policy 可配；后台 worker 每 5 分钟扫一次过期
5. **不破坏现有 API 形状**：`POST /transfers` 仍返回 `{id, status, ...}`，仅 status 取值多了几种；现有 `/execute` 行为对低额不变
6. **复用现有 RBAC**：MVP 不引入新角色，复用 `Admin = Checker` / `Operator = Maker` / Admin 可同时是 Maker（创建任意转账，但不能 approve 自己的）；Auditor 角色待 F1.1 落地后接入（read-only access to approval views）

### 3.3 策略匹配伪代码

```rust
// backend/src/services/approval_policy_service.rs
fn requires_approval(transfer: &TransferRequest, user: &User) -> Option<&ApprovalPolicy> {
    // 1. user-scoped policy (most specific)
    if let Some(p) = repo.find_user_policy(user.id, &transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    // 2. wallet-scoped policy
    if let Some(p) = repo.find_wallet_policy(transfer.wallet_id, &transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    // 3. global policy (default fallback)
    if let Some(p) = repo.find_global_policy(&transfer.chain, &transfer.token) {
        if transfer.amount >= p.amount_threshold { return Some(p); }
    }
    None
}
```

### 3.4 与 F1.1 / F3.1 的边界

- **F1.1（Viewing Key 审计层）**：Auditor 角色 + viewing key 暴露，read-only；本 PRD 不引入 Auditor，但策略表 `approval_policies` 允许 Auditor 后续以 read-only 访问
- **F3.1（Payroll Run 批量发薪）**：单 run 视为一组 `Transfer` 集合，每个 transfer 走本审批流；payroll_run 自身可加 "整 run 审批" 一级（F3.1 PRD 自行定义），不在本 PRD 范围

---

## §4 数据模型

> 严格遵守 CLAUDE.md C-2：所有 CREATE TABLE 由 `backend/src/db/migrations.rs` 启动时执行，不走 .sql 文件。

### 4.1 `Transfer` 状态枚举扩展

```rust
// backend/src/db/models.rs
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "VARCHAR")]
#[sqlx(rename_all = "snake_case")]
pub enum TransferStatus {
    Pending,                  // 现有，低额直接可执行
    AwaitingApproval,         // 新增：等待审批
    Approved,                 // 新增：审批通过，待执行
    Rejected,                 // 新增：审批拒绝（终态）
    Expired,                  // 新增：超时未审批（终态）
    Submitted,                // 现有
    Confirmed,                // 现有
    Failed,                   // 现有
}
```

### 4.2 `transfer_approvals` 表（新）

```sql
CREATE TABLE IF NOT EXISTS transfer_approvals (
  id INT PRIMARY KEY AUTO_INCREMENT,
  transfer_id INT NOT NULL,
  approver_user_id INT NOT NULL,
  decision VARCHAR(16) NOT NULL,        -- 'approve' | 'reject'
  reason TEXT,                          -- reject 必填；approve 可选备注
  policy_snapshot JSON,                 -- 决策时的策略快照（防策略改了找不回为何 approve）
  idempotency_key VARCHAR(128),
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_approval_idem (idempotency_key),
  UNIQUE KEY uq_one_decision_per_approver (transfer_id, approver_user_id),  -- NFR-7
  INDEX idx_transfer (transfer_id),
  INDEX idx_approver (approver_user_id)
);
```

### 4.3 `approval_policies` 表（新）

```sql
CREATE TABLE IF NOT EXISTS approval_policies (
  id INT PRIMARY KEY AUTO_INCREMENT,
  scope VARCHAR(16) NOT NULL,           -- 'global' | 'wallet' | 'user'
  scope_id INT,                         -- wallet.id 或 user.id, scope=global 时为 NULL
  chain VARCHAR(32) NOT NULL,
  token VARCHAR(32) NOT NULL,
  amount_threshold DECIMAL(36, 18) NOT NULL,
  sla_minutes INT NOT NULL DEFAULT 1440,    -- 默认 24 小时
  required_count INT NOT NULL DEFAULT 1,    -- v1=1, v2 N-of-M (F2.6)
  enabled TINYINT(1) NOT NULL DEFAULT 1,
  created_by INT NOT NULL,
  created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
  updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  UNIQUE KEY uq_scope_match (scope, scope_id, chain, token),
  INDEX idx_scope_lookup (scope, chain, token, enabled)
);
```

### 4.4 `transfers` 表增量字段

```sql
-- 后续 ALTER TABLE，启动 migration 时如果列不存在则添加
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS expiry_at TIMESTAMP NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approval_required TINYINT(1) NOT NULL DEFAULT 0;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approved_at TIMESTAMP NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS approved_by INT NULL;
ALTER TABLE transfers ADD COLUMN IF NOT EXISTS rejection_reason TEXT NULL;
```

> 💡 sqlx MySQL `IF NOT EXISTS` on ADD COLUMN 需 MySQL 8.0.29+；老版本走「先 SELECT INFORMATION_SCHEMA 再 ALTER」防御代码。`backend/src/db/migrations.rs` 实现时按现有版本自适应。

### 4.5 默认策略 seed（启动时仅当无策略存在则插入）

```rust
// 启动时由 migrations.rs 检查；只 seed 一次，后续 Admin 改不动它
INSERT INTO approval_policies (scope, chain, token, amount_threshold, sla_minutes, created_by)
SELECT 'global', 'ethereum', 'USDT', 5000, 1440, 1
WHERE NOT EXISTS (SELECT 1 FROM approval_policies WHERE scope='global' AND chain='ethereum' AND token='USDT');
-- 同样为 USDC / DAI / WETH / ETH / ZEC 各 seed 一条
```

---

## §5 API

### 5.1 修改：`POST /api/v1/transfers`

请求体不变。响应增加字段：

```json
{
  "id": 123,
  "status": "awaiting_approval",          // 新可能值
  "approval_required": true,              // 新字段，便于前端立刻判断
  "expiry_at": "2026-05-17T16:00:00Z",    // 新字段，若需审批则非空
  "matched_policy_id": 7,                 // 新字段，便于前端展示"为什么需要审批"
  ...
}
```

### 5.2 新增：`POST /api/v1/transfers/{id}/approve`

```http
POST /api/v1/transfers/123/approve
Authorization: Bearer <admin-jwt>
Idempotency-Key: <client-uuid>
Content-Type: application/json

{ "note": "Q2 财务季度奖金，已与 CEO 邮件确认" }
```

响应：

```json
{
  "transfer_id": 123,
  "approval_id": 456,
  "decision": "approve",
  "approver_user_id": 2,
  "approver_username": "alice",
  "approved_at": "2026-05-16T15:34:00Z",
  "transfer_status": "approved"
}
```

错误码：
- `403 NotAuthorized` — 调用者非 Admin
- `409 SelfApproveForbidden` — 调用者即 maker（NFR-7）
- `410 TransferExpired` — 已过 SLA
- `422 InvalidState` — 当前状态非 `AwaitingApproval`

### 5.3 新增：`POST /api/v1/transfers/{id}/reject`

```http
POST /api/v1/transfers/123/reject
Authorization: Bearer <admin-jwt>
Content-Type: application/json

{ "reason": "收款地址未通过 KYB 白名单" }
```

响应 / 错误码：同 `/approve`，外加 `400 ReasonTooShort`（reason < 5 字符）

### 5.4 修改：`POST /api/v1/transfers/{id}/execute`

行为变化：
- 当前状态为 `Pending`（无策略匹配 / 低额）→ 直接 Submitted，与现有行为一致
- 当前状态为 `Approved` → 校验 approver / SLA / approver ≠ executor（v2 可放宽，v1 严）→ Submitted
- 当前状态为 `AwaitingApproval` / `Rejected` / `Expired` → `422 InvalidState`

### 5.5 新增：`GET /api/v1/transfers/pending-approval`

返回当前用户**可审批**的转账列表（自动过滤自己作为 maker 的）：

```http
GET /api/v1/transfers/pending-approval?limit=20&cursor=eyJ0cyI6...
Authorization: Bearer <admin-jwt>
```

```json
{
  "items": [
    {
      "transfer_id": 123,
      "chain": "ethereum",
      "token": "USDT",
      "amount": "8500",
      "from_address": "0x..",
      "to_address": "0x..",
      "maker_username": "bob",
      "memo": "Vendor X invoice #2024-Q2-007",
      "matched_policy_id": 7,
      "expiry_at": "2026-05-17T16:00:00Z",
      "created_at": "2026-05-16T15:30:00Z"
    }
  ],
  "next_cursor": null
}
```

### 5.6 新增：`GET /api/v1/policies` · `PUT /api/v1/policies/{id}` · `POST /api/v1/policies`

Admin only。POST 创建策略 / PUT 更新 / GET 列表（含 scope 筛选）。

```http
POST /api/v1/policies
Authorization: Bearer <admin-jwt>

{
  "scope": "wallet",
  "scope_id": 42,
  "chain": "ethereum",
  "token": "USDT",
  "amount_threshold": "10000",
  "sla_minutes": 720,
  "required_count": 1
}
```

### 5.7 新增：`DELETE /api/v1/transfers/{id}` (FR-13 maker self-recall)

仅 `AwaitingApproval` 状态可调用，且只能由 maker 自己调用。其他状态返回 `422`。

### 5.8 Orchard 路径平行接口

- `POST /api/v1/transfers/orchard` 行为同 5.1（含审批字段）
- `POST /api/v1/transfers/orchard/{id}/execute` 同 5.4
- `POST /api/v1/transfers/orchard/{id}/approve` 等同 5.2-5.7 全套

> 💡 实现层：审批逻辑提到 `approval_service.rs`，被 ETH 和 Orchard 两个 transfer flow 共享，避免重复代码。

---

## §6 Frontend Spec（sweden owner）

### 6.1 涉及页面

1. **`pages/Transfer.tsx`（改造）** — 创建转账表单
2. **`pages/Transfer/Pending.tsx`（新）** — 我发起的待审批列表（maker 视角）
3. **`pages/Approval/Queue.tsx`（新）** — 待我审批队列（checker 视角）
4. **`pages/Approval/Detail.tsx`（新）** — 单审批详情 + 决定按钮
5. **`pages/History.tsx`（改造）** — 历史 + 审批轨迹时间线

### 6.2 状态机 UI（5 个 user-visible 状态）

```
┌──────────────────────────────────────────────────────────────┐
│  Page 1: Maker 创建转账                                       │
├──────────────────────────────────────────────────────────────┤
│  [创建表单] 填完点提交                                          │
│  ↓                                                             │
│  if amount ≥ matched policy.threshold:                         │
│    ⚠️ banner: "此转账需 Admin 审批（阈值 $5000，当前 $8500）"    │
│    submit → status = AwaitingApproval                          │
│    跳转 Page 2                                                  │
│  else:                                                          │
│    submit → status = Pending → 直接 /execute                    │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  Page 2: Maker 视角 — 我的审批中转账                          │
├──────────────────────────────────────────────────────────────┤
│  ⏳ 审批中 (3)                                                 │
│  ─────────────────────────────────────                         │
│  │ #123 USDT 8500 → 0xabc...   等待 Alice 审批  剩余 18h 30m │
│  │ [撤回] [复制详情]                                            │
│  ─────────────────────────────────────                         │
│  │ #124 ZEC 100 → u1xyz...     等待 Carol 审批 剩余 21h 02m │
│  ─────────────────────────────────────                         │
│  ✅ 已通过 (2)  ❌ 已拒绝 (1)  ⏰ 已超时 (0)                   │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  Page 3: Checker 视角 — 待我审批队列                          │
├──────────────────────────────────────────────────────────────┤
│  🔔 5 笔等你审批                                                │
│  ┌────┬────────┬────────┬─────────┬──────────┬────────────┐  │
│  │ ID │ 金额    │ Token   │ Maker    │ 剩余时间  │ 操作        │  │
│  ├────┼────────┼────────┼─────────┼──────────┼────────────┤  │
│  │123 │ 8500   │ USDT    │ bob      │ 18h 30m  │ [详情]      │  │
│  │125 │ 50000  │ ZEC     │ carol    │ 2h 15m ⚠️│ [详情] 🔴   │  │
│  └────┴────────┴────────┴─────────┴──────────┴────────────┘  │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  Page 4: Approval Detail (decision)                            │
├──────────────────────────────────────────────────────────────┤
│  Transfer #123                                                  │
│  Maker: bob                                                     │
│  金额: 8500 USDT  (~$8500)                                       │
│  收款方: 0xabc...def (供应商 X) 🏷                                │
│  Memo: "Vendor X invoice #2024-Q2-007"                          │
│  Matched policy: P-7 (ethereum USDT ≥ 5000)                     │
│  审批 SLA 剩余: 18h 30m                                          │
│                                                                  │
│  风险标签:                                                       │
│  ✓ 收款方在地址簿（F3.2，标签：供应商 X）                          │
│  ✓ Maker bob 历史 14 笔无失败                                    │
│  ⚠️ 单日累计已 $13,500 / 限额 $20,000                            │
│                                                                  │
│  [✅ 批准]  [❌ 拒绝]                                            │
│  备注（可选）: ___________________                                │
└──────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────┐
│  Page 5: 历史详情 — 审批轨迹时间线                            │
├──────────────────────────────────────────────────────────────┤
│  Transfer #123                                                  │
│  ──● 15:30 by bob — 创建（AwaitingApproval）                     │
│      │                                                          │
│  ──● 15:34 by alice — 批准 ✅ "Q2 财务季度奖金，已与 CEO 邮件确认"│
│      │                                                          │
│  ──● 15:35 by bob — 触发执行（Pending → Submitted）              │
│      │                                                          │
│  ──● 15:36 by system — 链上确认（tx_hash 0xab..）Confirmed ✅     │
└──────────────────────────────────────────────────────────────┘
```

### 6.3 状态对应配色

- 🟡 `awaiting_approval` — 等待审批
- 🟢 `approved` — 已批准
- 🔵 `pending` / `submitted` — 处理中
- 🟢 `confirmed` — 已确认
- 🔴 `rejected` / `expired` / `failed` — 失败/终止（区分 icon）

### 6.4 通知

- **toast** in-app（即时）：审批结果 / SLA 即将超时（剩余 ≤ 2h 时红色提醒）
- **email**（FR-14 stub）：审批请求 / 决定结果（后续接 F6.2）
- **mobile push**（M3+ F6.9）：CFO 在外 1-tap approve

### 6.5 i18n

- 中文键：`approval.required` / `approval.pending` / `approval.expired` / `approval.rejected_reason` / `approval.timeline.*`
- 英文键对应同名（已有 i18next 框架，添加 keys 即可）
- 西班牙文 / 日文 等扩展留 F3.8

### 6.6 与现有页面的兼容

- 现有 `pages/Transfer.tsx` 表单不破坏，仅在 submit 后根据响应 `approval_required` 跳转不同页面
- 现有 `pages/History.tsx` 增加新筛选：「我发起的 / 待我审批 / 已审批通过 / 已拒绝 / 已超时」
- 现有 Dashboard 增加 "待我审批 N 笔" 入口卡片（点击跳 Page 3）

---

## §7 里程碑

| 阶段 | 目标 | 验收 | 估计 |
|---|---|---|---|
| **D+0**（5/16，今天） | PRD-F2.1 push + Robust review | 三方 ack | 30 min |
| **D+1 ~ D+2**（5/17-5/18） | DB schema 增量 + migrations.rs + models 扩展 + repositories | `cargo build` 过 + 启动建表无误 | 1.5d |
| **D+3 ~ D+5**（5/19-5/21） | `approval_service.rs` + `approval_policy_service.rs` + 状态机 + 后台 SLA worker | 单元测试 + maker/checker = same user 阻止 | 3d |
| **D+6 ~ D+7**（5/22-5/23） | 7 个新 API + 修改现有 `/transfers` / `/execute` 行为 + Orchard 平行 | Postman 跑完 happy path + 4 个 error path | 2d |
| **D+8 ~ D+11**（5/24-5/27） | 前端 5 个页面 + i18n + 状态机 UI + 时间线 | 前后端联调通 | 4d |
| **D+12 ~ D+13**（5/28-5/29） | 集成测试 + 与 F1.1 / F3.1 协同（共享 audit_logs / approval workflow） | 端到端 + cross-check | 2d |
| **D+14 ~ D+15**（5/30-5/31） | bug fix + UI 打磨 + 文档（README + API 示例） | RC 候选 | 2d |
| **D+16 ~ D+18**（6/1-6/3） | smoke 上 staging + UAT | 客户 demo pass | 3d |
| **D+19 ~ D+21**（6/4-6/6） | 上 prod | smoke + 关键告警就位 | 3d |

> M1 共 21 个工作日，三 P0 并行，F2.1 是其中线程之一。F1.1 / F3.1 同 timeline。

---

## §8 风险

| ID | 风险 | 等级 | 当前缓解 |
|---|---|---|---|
| RK-1 | **现有自动化客户端被破坏** — 老客户的 `POST /transfers` + `/execute` 一气呵成脚本碰到 `AwaitingApproval` 状态会卡死 | 🟠 中 | NFR-2 向后兼容 — 默认 global 阈值 seed 较高（$5k+），低额客户不受影响；新客户走 docs 改造；客户响应中 `approval_required: true` 字段让脚本检测 |
| RK-2 | **NFR-7 maker = checker 漏洞** — Admin 创建自己审批 | 🔴 高 | DB unique 索引 + 应用层 `transfer.created_by ≠ approver.user_id` 双检；审批前 `transfer_approvals` 表写入时 transaction 校验 |
| RK-3 | **SLA worker 漏单** — worker 挂了 expiry 没切，转账卡 AwaitingApproval | 🟠 中 | worker 持有 cron-style schedule + Prometheus exporter（NFR-5）告警；冷启动时一次性扫所有过期单 |
| RK-4 | **大额转账阻塞业务** — checker 周末没看到，业务停摆 | 🟡 低 | SLA 默认 24h；F6.2 email 通知 + F6.9 mobile push（M3-M5）；客户可调 sla_minutes per policy；FR-13 maker 可撤回重发 |
| RK-5 | **审批策略 race condition** — Admin 改了策略，已 AwaitingApproval 的转账按哪个版本？ | 🟡 低 | `approval_policies` 修改不溯及既往；已写入 `transfer.matched_policy_id` 锁定决策依据；`policy_snapshot` JSON in `transfer_approvals` 留证 |
| RK-6 | **Orchard 路径未一并改造** — 漏掉 `/transfers/orchard/{id}/execute` 的审批校验，绕过审批 | 🔴 高 | `approval_service.rs` 设计为 chain-agnostic，被 ETH / Orchard handler 共享调用；测试 case 覆盖两条路径 |
| RK-7 | **idempotency-key 复用风险** — 同 key 不同决定怎么处理 | 🟢 低 | DB `uq_approval_idem` 唯一约束 + 返回原始决定（不报错），客户端体验"重试成功" |
| RK-8 | **F1.1 Auditor 角色与 F2.1 view 权限冲突** — Auditor 能看 pending-approval 列表吗？ | 🟢 低 | 双 PRD 联调期对齐：Auditor read-only 可看，不能 approve / reject；handler middleware 区分 |

---

## 附录 A · 与 F1.1 / F3.1 的接口约定

- F1.1 落地 `Auditor` 角色后，本 PRD 5.5/5.6 接口加 Auditor read-only 路径（GET 允许，POST/PUT/DELETE 拒绝）
- F3.1 落地 `payroll_runs` 后，**整 run 的审批走「run 级别 maker/checker」**（在 F3.1 PRD 定义），不强求每个 `payroll_item` 单独审批；run-level approval 完成后，run 内各 item 转 `Approved` 批量执行
- 共享 `audit_logs.action` 命名空间：`transfer.create_pending_approval` / `transfer.approve` / `transfer.reject` / `transfer.expire` / `transfer.execute_with_approval` / `policy.create` / `policy.update`

## 附录 B · 编辑历史

- 2026-05-16 sweden — 初版 v0.1，§1-§8 全部
