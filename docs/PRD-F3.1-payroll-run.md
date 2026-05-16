# PRD-F3.1 — 批量发薪 Payroll Run (纯 ZEC, no swap)

> **状态**：Draft v0.1 · 2026-05-16
> **分支**：`feat/m1-2026-06`
> **属于**：M1 (2026-06) P0 主线 3/3 · 可用性维度 (D3)
> **主笔**：france 🥖（§1-5/§7-8）· sweden 🇸🇪（§6 Frontend spec）
> **关联**：`docs/feature-backlog-2026.md` §3.3 F3.1 · `docs/zcash-enterprise-use-cases-en.md` §8

> **一句话**：把 `docs/zcash-enterprise-use-cases-en.md` §8 「Payroll Distribution」的脚本示意做成**真后端实体**：CSV 导入员工 → 一笔 Orchard tx 多 output fan-out 给 N 个员工 u-addr → 失败重试 → 状态可查。**纯 ZEC**，不带跨币种 swap（5/16 已 freeze）。

---

## §1 现状

- `docs/zcash-enterprise-use-cases-en.md` §8 已有 payroll 业务流描述，**但只是串行调脚本**示意，不是后端 entity
- 现有 `transfers` 表是**单腿**记账（一条记录 = 一笔链上转账），无「批次」概念
- `POST /api/v1/transfers/orchard` 接口当前 **单笔单收款人**，无多 output fan-out 路径
- librustzcash / Orchard 0.13 内部支持 multi-output bundle（`OrchardTransactionBuilder::add_recipient` 等），但 service 层没暴露
- 无员工档案、无 CSV 导入、无批次失败重试机制

---

## §2 需求

### 2.1 用户故事

> **As** 企业 HR / 财务
> **I want to** 月底导一份 CSV（员工名 + u-addr + ZEC 金额 + memo），点一下按钮全部发出去
> **so that** 不用一笔一笔手动操作 50 个员工，**而且员工之间互相看不到工资金额**（同 tx 内 fan-out 提高 anonymity set）

### 2.2 功能性需求 (FR)

| ID | 需求 | 优先级 |
|---|---|---|
| F3.1.1 | 新 `employees` 表：employee_code / name / wallet_address / chain / tags JSON / active. **tags JSON 容纳 M2 字段**（preferred_token / privacy_mode / kyc_status 等），M1 不平铺到独立列以保留 schema 灵活性 | P0 |
| F3.1.2 | 新 `payroll_runs` 表：批次（pay_period / source_wallet_id / total_amount / status / 等） | P0 |
| F3.1.3 | 新 `payroll_items` 表：批次单条（run_id / employee_id / address / amount / memo / status / tx_hash） | P0 |
| F3.1.4 | API 创建 payroll_run + 上传 items（CSV / JSON 两种格式） | P0 |
| F3.1.5 | CSV 上传**预校验**：地址合法性 / amount > 0 / employee 是否存在 / 总额 ≤ wallet 余额 | P0 |
| F3.1.6 | API 执行 payroll_run：**单笔 Orchard tx 多 output fan-out** 给所有 item 收款人 | P0 |
| F3.1.7 | 超过 single-tx output 上限时**自动分批**（multiple Orchard tx） | P0 |
| F3.1.8 | API 查 payroll_run 状态：单 item 级 tx_hash / 链上确认状态 / 失败原因 | P0 |
| F3.1.9 | 部分失败处理：单 tx 失败不阻塞其他 tx，整 run 状态 `partial_success` | P0 |
| F3.1.10 | 单 item 失败可**单独重试**（不重发整 run） | P0 |
| F3.1.11 | 报表导出（CSV）：含 employee_id / amount / tx_hash / status / 时间戳，**金额加密以雇主 viewing key 解锁**（链上仍隐私） | P0 |
| F3.1.12 | 整 run 进 `audit_logs` + 单 item 重要操作（重试 / 取消）也进 | P0 |
| F3.1.13 | maker/checker 集成（F2.1 落地后）：payroll_run 执行需双签 | P1 (M1 末) |

### 2.3 非功能性需求 (NFR)

| ID | 需求 |
|---|---|
| NFR-1 | 单 run 上限：**100 员工**，超过分多 run（M1 limit，M2 优化到 500） |
| NFR-2 | 单 Orchard tx output 上限：sub-agent W1 实测 librustzcash 限制，默认 **16 outputs/tx** （ZIP-317 fee tier） |
| NFR-3 | Orchard fan-out tx **隐私保护**：单 tx 内 N 个 output 增大 anonymity set，员工互相不可关联（NFR-1 zpay PRD 同款） |
| NFR-4 | **原子性**：单 tx 内的 outputs 要么全成功要么全失败（链上保证）；多 tx 分批时**逐个 commit DB**，避免单 tx 挂导致全 run rollback |
| NFR-5 | **审计**：每个 tx 广播前后 + 单 item 状态变更全写 `audit_logs` |
| NFR-6 | **不破坏现有 API**：现有 `/transfers/orchard` 保持不变，新功能走 `/payroll/*` 前缀 |

### 2.4 OOS (Out of Scope, M1)

- ❌ **多币种 swap**（USDT/USDC/ETH → ZEC）— Robust 5/16 freeze
- ❌ **Streaming payroll**（Sablier 风格）— 一次性批量
- ❌ **税务计算 / 工资单 PDF 法律模板** — 外部系统
- ❌ **员工自助门户**（员工自己看 payslip）— M2 加（F3.x）
- ❌ **多 ETH 链 fan-out** — 仅 ZEC（ETH 没有 single-tx multi-output native 概念，要逐笔走，意义不大）

---

## §3 技术方案

### 3.1 主流程

```
[Admin POST /payroll/runs (CSV)]
        ↓
PayrollService.create_run(source_wallet_id, csv_file)
        ↓
csv parser → 校验地址 + amount + 员工存在性 + 总额 ≤ 余额
        ↓
写 payroll_runs (status=pending) + payroll_items (status=pending)
        ↓
返回 run_id + 预校验结果（失败 items 列表）

[Admin POST /payroll/runs/{id}/execute]
        ↓
PayrollService.execute_run(run_id)
        ↓
按 NFR-2 max_outputs_per_tx 拆分 items 成 N 个 tx-group
        ↓
foreach tx-group:
    OrchardTransactionBuilder.add_recipient(addr, amount, memo) × items
    build + sign + broadcast
    if success: 标记 group 内所有 items status=submitted, tx_hash=...
    if fail: 标记 group 内所有 items status=failed, error=...
        ↓
轮询 chain confirm → items.status: submitted → confirmed (block_number 写入)
        ↓
整 run 终态：success / partial_success / failed
```

### 3.2 复用现有代码点 inventory

| 现有 | 新增/复用 |
|---|---|
| `OrchardTransactionBuilder` (`backend/src/blockchain/zcash/orchard/`) | **直接复用**，加 `add_multiple_recipients()` helper |
| `transfer_service.rs` 中 Orchard 单笔流程 | 抽出 `build_orchard_tx()` 函数供 payroll 调用 |
| `transfers` 表 | 单 item 转账完成时**写一条 transfer 记录**（保持向后兼容），加 `payroll_item_id` 外键 |
| `WalletService.get_active_wallet()` | 复用获取 source wallet |
| `audit_logs` | 复用 |

### 3.3 CSV 格式

```csv
employee_id,name,address,amount_zec,memo
EMP-001,Alice,u1qwerty...alice,3.85,2026-05 salary
EMP-002,Bob,u1qwerty...bob,4.62,2026-05 salary
EMP-003,Charlie,u1qwerty...charlie,1.15,2026-05 bonus
```

- 列固定，header 必须
- 每行最大 512 byte memo
- 总行数 ≤ NFR-1 (M1: 100)

### 3.4 多 tx 分批逻辑

伪代码：

```rust
const MAX_OUTPUTS_PER_TX: usize = 16;  // NFR-2, configurable in .env

let batches: Vec<Vec<PayrollItem>> = items
    .chunks(MAX_OUTPUTS_PER_TX)
    .map(|c| c.to_vec())
    .collect();

for batch in batches {
    match build_and_broadcast(batch) {
        Ok(tx_hash) => mark_items_submitted(batch, tx_hash),
        Err(e) => mark_items_failed(batch, e),
    }
}
```

---

## §4 数据模型

> 按 CLAUDE.md C-2，**Rust + sqlx 启动时自动建表**。

### 4.1 `employees`

```sql
CREATE TABLE IF NOT EXISTS employees (
  id INT AUTO_INCREMENT PRIMARY KEY,
  employee_code VARCHAR(64) NOT NULL UNIQUE,    -- 业务侧员工 ID (EMP-001 等)
  name VARCHAR(255) NOT NULL,
  wallet_address VARCHAR(255) NOT NULL,         -- u1... 或 t1...
  chain VARCHAR(32) NOT NULL DEFAULT 'zcash',
  tags JSON NULL,                                -- ["engineering", "fulltime"]
  active BOOLEAN NOT NULL DEFAULT TRUE,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  INDEX idx_active (active),
  INDEX idx_address (wallet_address)
);
```

### 4.2 `payroll_runs`

```sql
CREATE TABLE IF NOT EXISTS payroll_runs (
  id INT AUTO_INCREMENT PRIMARY KEY,
  pay_period VARCHAR(32) NOT NULL,              -- "2026-05" / "2026-Q2-bonus"
  source_wallet_id INT NOT NULL,
  total_amount DECIMAL(28, 8) NOT NULL,
  item_count INT NOT NULL,
  status VARCHAR(24) NOT NULL DEFAULT 'pending',  -- pending|executing|success|partial_success|failed|cancelled
  created_by_user_id INT NOT NULL,
  executed_by_user_id INT NULL,                 -- maker/checker: maker = created, checker = executed
  executed_at DATETIME NULL,
  notes TEXT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  FOREIGN KEY (source_wallet_id) REFERENCES wallets(id),
  FOREIGN KEY (created_by_user_id) REFERENCES users(id),
  INDEX idx_status (status),
  INDEX idx_period (pay_period)
);
```

### 4.3 `payroll_items`

```sql
CREATE TABLE IF NOT EXISTS payroll_items (
  id INT AUTO_INCREMENT PRIMARY KEY,
  run_id INT NOT NULL,
  employee_id INT NULL,                         -- nullable: 一次性 ad-hoc 收款可不绑员工
  employee_address VARCHAR(255) NOT NULL,
  amount DECIMAL(28, 8) NOT NULL,
  memo TEXT NULL,                               -- 512 byte 上限
  status VARCHAR(24) NOT NULL DEFAULT 'pending',  -- pending|submitted|confirmed|failed
  tx_hash VARCHAR(128) NULL,
  block_number BIGINT NULL,
  transfer_id INT NULL,                         -- 关联 transfers 表写入的副本
  error_message TEXT NULL,
  retry_count INT NOT NULL DEFAULT 0,
  last_attempt_at DATETIME NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
  FOREIGN KEY (run_id) REFERENCES payroll_runs(id) ON DELETE CASCADE,
  FOREIGN KEY (employee_id) REFERENCES employees(id),
  FOREIGN KEY (transfer_id) REFERENCES transfers(id),
  INDEX idx_run_status (run_id, status),
  INDEX idx_tx_hash (tx_hash)
);
```

> 💡 注意 `transfers` 表加 `payroll_item_id INT NULL` 外键，单 item 上链时**追写一条 transfer 记录**保持向后兼容现有 list/detail API。

---

## §5 API 设计

| Method | Path | Body | Resp |
|---|---|---|---|
| POST | `/payroll/employees` | `{employee_code, name, wallet_address, chain, tags}` | `{employee_id}` |
| GET | `/payroll/employees` | `?active=&q=` | `[employee...]` |
| PUT | `/payroll/employees/{id}` | partial | `{ok}` |
| DELETE | `/payroll/employees/{id}` | — | `{ok}` (soft delete, set active=false) |
| POST | `/payroll/runs` | multipart: `csv_file` + `{pay_period, source_wallet_id, notes}` | `{run_id, item_count, validation_errors[]}` |
| POST | `/payroll/runs` (JSON) | `{pay_period, source_wallet_id, items[], notes}` | 同上 |
| GET | `/payroll/runs` | `?status=&pay_period=&limit=&offset=` | `[run summary...]` |
| GET | `/payroll/runs/{id}` | — | `{run_meta, items[]}` |
| POST | `/payroll/runs/{id}/execute` | — | `{ok, batch_count}` (异步执行) |
| POST | `/payroll/runs/{id}/cancel` | — | `{ok}` (仅 pending 状态可取消) |
| POST | `/payroll/runs/{id}/items/{item_id}/retry` | — | `{ok, new_tx_hash?}` (仅 failed 状态可) |
| GET | `/payroll/runs/{id}/report?format=csv` | — | binary CSV |

### 5.1 错误码

- `400 INVALID_CSV` — header 缺失 / 列数不对
- `400 INVALID_ADDRESS` — items 列表里有非法 ZEC 地址
- `400 INSUFFICIENT_BALANCE` — wallet 余额 < total_amount
- `409 RUN_NOT_PENDING` — execute 时 run 已是 executing/success/failed 等态
- `429 RUN_ITEM_LIMIT` — items > NFR-1 上限
- `503 ORCHARD_RPC_DOWN` — 链节点不可达（暂时性）

---

## §6 Frontend Spec — sweden owner

<!-- ============================================================ -->
<!-- sweden:6 — Payroll Run 前端 spec, 同 PRD-F1.1 §6 同款占位规则 -->
<!-- 建议结构:                                                     -->
<!-- §6.1 员工档案管理页 (CRUD + 批量 CSV 导入员工)                  -->
<!-- §6.2 创建 Payroll Run 向导（上传 CSV → 预校验显示 → 确认创建）  -->
<!-- §6.3 Run 列表 + 状态徽章 + 筛选                                -->
<!-- §6.4 Run 详情页（item 表格 + 单 item 状态 + 失败重试按钮）      -->
<!-- §6.5 执行按钮 + 进度 polling + maker/checker 集成 placeholder  -->
<!-- §6.6 报表导出按钮 + CSV preview                                -->
<!-- §6.7 i18n 中英文 key 命名                                       -->
<!-- ============================================================ -->

---

## §7 里程碑（4 周，2026-05-17 ~ 2026-06-13）

| 周 | 日期 | 后端 (france) | 前端 (sweden) | 验收 |
|---|---|---|---|---|
| **W1** | 5/17-5/23 | DB schema 3 表 auto-migrate / employees CRUD API + repo / CSV parser + 预校验逻辑 / sub-agent 实测 librustzcash `add_recipient` multi-output 上限 | 员工档案 CRUD 页面骨架 / CSV 上传组件 | curl 走通 employee CRUD + run 创建 + 校验失败展示 |
| **W2** | 5/24-5/30 | `PayrollService::execute_run` 核心：单 tx multi-output fan-out / 多 tx 分批 / 错误处理 + 重试逻辑 / 写 transfer 副本保持兼容 | Run 列表 + 详情页 / 状态徽章 / 执行按钮 + 异步 polling | Postman 走完 11 个 API + 单 run 5 员工真链上 fan-out |
| **W3** | 5/31-6/6 | 失败单 item 重试 / report CSV 导出 / maker/checker 钩子（F2.1 集成）/ tx confirm 后台 worker | item 表格 / 重试 UX / 报表下载 / i18n 中英 | e2e 真链上：导 CSV → 执行 → 部分失败 → 重试 → confirm → 报表 |
| **W4** | 6/7-6/13 | bug list / 100 员工压测 / dev 部署 | 移动端响应式 / 错误文案 polish / 极端 case | smoke 8 场景 + prod 候选 |

---

## §8 风险登记

| ID | 风险 | 等级 | 缓解 |
|---|---|---|---|
| R1 | **librustzcash Orchard `add_recipient` multi-output 上限**（理论 supports 多 outputs，但 Halo 2 proof 时长 + tx size 限制实际多少？） | 🔴 高 | W1 sub-agent 实测，默认配置 16 outputs/tx，超过分批 |
| R2 | Orchard fan-out tx 失败回滚原子性（单 tx 内 N outputs 要么全成要么全失，OK；但分多 tx 时部分挂） | 🟠 中 | NFR-4 多 tx 分批时**逐个 commit DB**，整 run partial_success 是合法终态 |
| R3 | 员工地址错误导致 burned funds（不可逆） | 🔴 高 | 预校验严格：`validate_address` + 历史 disclose 历史地址匹配（M2 加 KYB 二次确认） |
| R4 | 大额 payroll 触发资金风控（合规） | 🟠 中 | NFR-1 单 run 100 员工 + F2.1 maker/checker 双签（M1 末集成） |
| R5 | 100 员工压测时 Halo 2 proof 生成耗时（5 分钟+？） | 🟠 中 | W3 实测，超过 30s 则前端必须异步 polling，UI 必备进度 |
| R6 | mainnet 节点不可达（见 PRD-F1.1 R6） | 🟡 低 | 同 F1.1 R6 处理 |
| R7 | CSV 编码 / BOM / 中文乱码 | 🟡 低 | csv crate UTF-8 强制 + BOM 自动剥离 + 文档说明 |

---

## 附录 A · 编辑历史

- 2026-05-16 france — 初版 §1-§5 + §7-§8，§6 frontend 留 sweden 占位

## 附录 B · 术语表

- **payroll_run** — 一次批量发薪批次实体
- **payroll_item** — 批次内单个员工发薪条目
- **fan-out** — 单 Orchard tx 内多 output 同时发给多个收款人
- **partial_success** — run 终态之一，部分 items 链上确认部分失败
