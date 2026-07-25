# zPay Enterprise 用户操作手册

> 面向企业财务、合规、审计角色的端到端操作指南。
> 配套 staging 环境: <https://staging.example.com>
> 技术细节请见 [PRD 文档](.) 与 [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md)。
> English version: [USER-MANUAL-EN.md](USER-MANUAL-EN.md)

---

## 目录

- [一、文档导览与角色定义](#一文档导览与角色定义)
- [二、首次部署后必做](#二首次部署后必做)
- [三、审计与合规](#三审计与合规)
- [四、员工与批量发薪](#四员工与批量发薪)
- [五、端到端业务场景演练](#五端到端业务场景演练)
- [六、故障排查与常见问题](#六故障排查与常见问题)
- [七、安全建议与推广要点](#七安全建议与推广要点)
- [八、Ironwood 迁移与批量隐私转账（F4）](#八ironwood-迁移与批量隐私转账f4)

---

## 一、文档导览与角色定义

### 1.1 这个系统是干什么的

zPay Enterprise 是一套**企业级 Web3 钱包 + 发薪 + 审计**一体化平台，主打三件事：

1. **批量发薪** — CFO/HR 每月给 N 名员工发工资 / 项目奖金，从手工 N 笔点击变成一次 CSV 上传 + 一次按钮点击
2. **双签风控** — 大额转账自动进入审批队列，**maker ≠ checker**（发起人不能批准自己），防止内部人单点风险
3. **隐私合规审计** — 外部审计师能在不拿私钥的前提下，看到指定钱包的真链上历史 + 一次性披露报告，符合监管要求

支持链：以太坊（ETH / ERC20）+ Zcash（透明 + 屏蔽）

### 1.2 三种用户角色

```
┌─────────────────────────────────────────────────────────────────┐
│  Admin (管理员)                                                  │
│  ├─ 创建钱包、配置审批策略、邀请审计师                            │
│  ├─ 所有 Operator 能做的事                                       │
│  └─ 不能看 Auditor 视角（双 JWT 物理隔离）                       │
├─────────────────────────────────────────────────────────────────┤
│  Operator (操作员，M1 默认 = Admin)                              │
│  ├─ 发起转账、发起批量发薪                                       │
│  ├─ 审批别人发起的转账（前提：自己不是发起人）                    │
│  └─ 不能改审批策略、不能创建钱包                                 │
├─────────────────────────────────────────────────────────────────┤
│  Auditor (审计师)                                                │
│  ├─ 独立登录入口 /auditor/login                                  │
│  ├─ 只能看 Admin 授权的钱包，且仅 scope 时间窗内                  │
│  ├─ 申请披露报告（PDF / CSV / JSON）                             │
│  └─ 完全只读，不能转账、不能改配置                               │
└─────────────────────────────────────────────────────────────────┘
```

**关键设计**：Admin 与 Auditor 的 JWT **物理隔离**（不同 `kind` 字段），互相不能跨视角访问。审计师必须用独立账号登录 `/auditor/login`，Admin 想看审计师视角必须 sign out 再以审计师身份重新登录。

---

## 二、首次部署后必做

### 2.1 首次登录

部署完成后，访问 <https://staging.example.com>（或你的部署域名），登录页：

- 用户名：`admin`
- 密码：首次启动时由后端自动生成，写在服务器上 `backend/.env.secrets`（chmod 0600，仅运维可读）
- 部署运维通过私信 / 1Password / Vault 等安全渠道把初始密码交给你

⚠️ **必做：登录后第一件事改密码**
- 右上角 `admin` → Change Password → 新密码 ≥ 12 字符 + 数字字母混合
- 改完之后 `.env.secrets` 里的初始密码就失效了（仅作为 bootstrap）

### 2.2 创建钱包

侧栏 **区块链** → **钱包列表** → **新建钱包**

| 字段 | 说明 |
|---|---|
| 名称 | 内部识别名，例：`公司主账户-ZEC` / `项目A 财务-USDT` |
| 链 | ethereum 或 zcash 二选一 |

后端会生成私钥并 AES-GCM 加密存储（永不明文落盘）。**重要**：
- 钱包私钥**只能通过 Export Private Key 流程导出一次**（要求二次输入 admin 密码做 fresh intent 验证）
- 备份建议：导出后立即写入硬件钱包 / 离线 USB，**不要存网盘**

### 2.3 激活钱包

每条链只能有 **一个 active 钱包**，发起转账时默认从 active 钱包出。

- 钱包列表 → 你想用的钱包 → **设为活跃**
- 钱包详情可看：余额（透明 / shielded 双值，Zcash 钱包）/ 转账历史 / Orchard 同步进度

### 2.4 (可选) 给钱包打第一笔种子资金

测试环境：用别的钱包给 staging 钱包打一笔小金额（ZEC: 0.001 / ETH: 0.001），等链上确认后侧栏余额会更新。

正式环境：通过现有公司钱包 / 交易所提现到这个 Active 钱包地址。

---

## 三、审计与合规

### 3.1 配置审批策略（什么转账要双签）

侧栏 **治理与合规** → **审批策略** → **新建策略**

```
┌────────────────────────────────────────────────────────┐
│  范围 (scope) ：global | wallet | user                  │
│  ├─ global  = 所有钱包都生效                             │
│  ├─ wallet  = 仅特定钱包出账时生效                       │
│  └─ user    = 仅特定 user 发起时生效                    │
├────────────────────────────────────────────────────────┤
│  链 + 币种   ：例 ethereum + USDT                       │
├────────────────────────────────────────────────────────┤
│  金额阈值    ：例 1000 (USDT)                           │
├────────────────────────────────────────────────────────┤
│  SLA 分钟数  ：例 120 (2 小时不批就 expired)            │
│  审批人数    ：M1 = 1 ; M2 支持 N-of-M                  │
└────────────────────────────────────────────────────────┘
```

**匹配优先级**（同一条转账可能命中多个策略）：
1. **user 策略** 优先于 wallet 策略
2. **wallet 策略** 优先于 global 策略
3. 同 scope 内：取**阈值最小**且金额已超的那条

**常见配置示例**：

| 业务诉求 | 策略配置 |
|---|---|
| 所有 USDT 大额转账双签 | `global / ethereum / USDT / 1000 / SLA 120 / 1人批` |
| 仅财务钱包大额 ZEC 双签 | `wallet=5 / zcash / ZEC / 10 / SLA 240 / 1人批` |
| 新员工高风险，所有发起都要审 | `user=42 / ethereum / USDT / 0.01 / SLA 60 / 1人批` |
| 完全不要审批 | 不配任何策略 = 所有转账直接执行 |

### 3.2 审批流程（双签场景）

```
 Maker (发起人)                  System                  Checker (审批人)
─────────────────              ──────────              ──────────────────
1) 发起转账 100 USDT  ───────►  匹配策略 / 阈值 800
                                │
                                ├─ 不命中 → 直接执行
                                │
                                └─ 命中 → 改 status            收到通知（M2）
                                   awaiting_approval ────────► 待审批队列出现该条
                                   记录 expiry_at
                                                                │
                                                                ├ Approve（写可选备注）
                                                                │  → status: approved
                                                                │
                                                                └ Reject（强制写理由 ≥ 5字符）
                                                                   → status: rejected
                                                                
                                 ┌─────────────────────────────────────────┐
                                 │  SLA Worker (每 5 分钟扫一次)            │
                                 │  超 expiry_at 未批的自动 flip "expired"  │
                                 └─────────────────────────────────────────┘

3) Approved 后 maker 点 Execute  →  真链上发送  →  status: confirmed
```

**关键规则（NFR-7）**：
- **maker ≠ checker** — 后端 SQL 层硬过滤 `WHERE initiated_by <> viewer_user_id`，即使 frontend 出 bug 漏过 RBAC，DB 层也拦住
- 因此**单 user 环境无法演示双签**——必须创建第二个 user 账号当审批人

### 3.3 邀请审计师

侧栏 **治理与合规** → **审计师管理** → **邀请审计师**

| 字段 | 说明 |
|---|---|
| 邮箱 | 审计师邮箱（仅作为登录用户名，系统不发邮件） |
| 姓名 | 显示用 |
| 可见钱包 | 多选你授权他看的钱包 |
| 时间窗 start / end | 仅这段时间内的链上数据他能查到（例 2026-Q1） |
| 披露次数预算 | 例 10 次 / 季 — 防滥用 |

**提交后系统返回**：
- `temp_password`（临时密码，24h 内必须使用 + 一次性登录后失效）
- 一个"在新标签打开审计师登录页"按钮

**重要安全规则**：
- temp_password **只在创建后一次性显示**，关闭弹窗后系统不保留明文（只存 hash）
- 通过**私信 / 1Password / Vault** 转交给审计师，**不在 Discord 群 / 邮件 / 截图明文发**
- 审计师首次登录后立即要求改密码

### 3.4 导出 Viewing Key（让审计师独立查 ZEC 历史）

审计师只看 zPay UI 不够 — 他想用自己的 Zashi 钱包独立验证，需要 viewing key。

侧栏 **钱包列表** → 选 zcash 钱包 → **Export Viewing Key**

| 选项 | 谁会用 | 说明 |
|---|---|---|
| **OVK** (Outgoing Viewing Key) | 只想看本钱包**发出**的交易 | 信息量最小 |
| **IVK** (Incoming Viewing Key) | 只想看本钱包**收到**的交易 | 中等 |
| **UFVK** (Unified Full Viewing Key) | 完整审计视角，收+发都看 | 信息最全；ZIP-316 标准 `uview...` 字符串可直接粘贴进 Zashi |

**流程**：
1. 点 Export → 再次输入 admin 密码（fresh intent 防 JWT 泄露）
2. 系统返回 24h 有效的下载 URL（base64url token）
3. 把 URL 私信给审计师
4. 审计师**点开一次**就消耗 token（one-time）；第二次访问返 410 Gone
5. 审计师把 `uview...` 字符串粘进 Zashi 即可看完整历史

**审计 log**：每次 export 都在 `viewing_key_exports` 表留痕（谁导出、什么时候、IP、key 哈希），即使 token 已消耗也能追溯。

### 3.5 审计师视角操作（审计师自己看）

审计师拿到 email + temp_password 后：

1. 访问 <https://staging.example.com/auditor/login>（**注意 URL 不一样**，独立入口）
2. 登录后进 **Auditor Dashboard**，看到 11 个字段的钱包列表卡片：
   - 钱包名 / 地址 / 链
   - scope 时间窗
   - 披露预算 `current/max`
   - tx 总数 / 最后活动时间 / 进行中披露数
3. 点钱包 → **WalletDetail** → 看：
   - 真链上余额（透明 + shielded 双值）
   - scope 时间窗内的真历史 transfers（分页）
4. 点 **申请披露** → 选 granularity 与 format：

```
granularity:
├─ tx          → 单笔交易详情（输入 tx_hash）
├─ address     → 该钱包全历史（涵盖 scope 窗内所有 incoming notes）
└─ range       → 时间段（输入起止日期，自动转 block height）

format:
├─ json   → 完整结构化数据（程序员友好）
├─ csv    → 10 列固定表头（Excel 友好）
└─ pdf    → A4 单页 + 表格（监管 / 打印归档）
```

5. 提交后状态 `generating`，前端自动每 2 秒轮询，通常 < 1 秒变 `ready`
6. 点 **下载** → 浏览器弹出 PDF / CSV / JSON

**披露报告内容**（ZIP-307 inspired enterprise body）：
- 每条交易：tx_hash / 区块高度 / 金额 (ZEC + zatoshis 双值) / memo / **revealed nullifier**（审计师可独立链上验真）/ 是否已花费
- 头部：钱包地址 / granularity / 范围解析（同时给 block height + 时间戳）

---

## 四、员工与批量发薪

> 本章节面向 CFO / HR / 财务执行人。批量发薪是日常高频操作，目标是把每月 N 名员工的工资从手工 N 次点击压成"一次上传 + 一次确认"。

### 4.1 员工花名册（一次性配好，长期复用）

侧栏 **发薪** → **员工**

发薪前必须先把员工档案录进系统。系统会按 `employee_code`（工号）+ 钱包地址唯一识别员工，发薪 CSV 里只填 `employee_code` 就能引用，避免每次重写地址。

| 字段 | 说明 | 例 |
|---|---|---|
| 员工编号 (employee_code) | 你公司内部工号，全局唯一 | `E001` / `ENG-042` |
| 姓名 | 显示用 | 王小明 |
| 钱包地址 | 员工自己提供的收款地址 | `u1...`（zcash）或 `0x...`（eth） |
| 链 (chain) | 该员工默认收款链 | zcash（推荐）或 ethereum |
| 标签 (tags JSON) | 自由结构，存部门 / 职级 / KYC 状态 / 默认币种 等 | `{"dept":"工程","kyc":"verified"}` |
| 启用 (active) | false 后该员工不会出现在发薪选择器里，但历史记录保留 | true |

**录入方式**：
1. **单个新建** — 点 New，逐字段填，适合 onboard 单个新员工
2. **CSV 批量导入** — 准备一份 `code,name,wallet_address,chain,tags` 的 CSV，一次性导 N 行（M1 通过新建表单粘贴或后续补 import endpoint）
3. **Edit / soft delete** — 改字段实时生效；Delete 是软删（数据保留 + active 翻 false），关联的历史发薪记录全部留痕，**永远不会真删**

> 💡 **设计意图**：员工不是一次性的 — 离职后软删，发薪历史还在；KYC 状态等存 `tags JSON`，加新字段不用动数据库 schema。

### 4.2 发薪审批策略（按月度总额触发双签）

发薪是大额操作，建议给发薪用的钱包专门配一条审批策略（详见 §3.1）：

```
┌───────────────────────────────────────────────────────────────┐
│  典型企业配置                                                  │
├───────────────────────────────────────────────────────────────┤
│  scope:        wallet=<公司发薪 ZEC 钱包 ID>                  │
│  chain+token:  zcash + ZEC                                    │
│  amount:       1000 ZEC   ← 月度发薪总额超 1000 ZEC 触发审批  │
│  SLA:          240 min    ← 4 小时内必须批，否则 expired       │
│  required:     1 (M1)                                         │
└───────────────────────────────────────────────────────────────┘
```

**关键细节**：发薪触发审批是按 **整批总额** 比较，不是按单个员工金额。比如 50 名员工每人 0.5 ZEC，总额 25 ZEC，跟策略阈值 20 ZEC 比 → 触发；不是 50 笔单独审批（避免审批员被 50 个通知淹没）。

### 4.3 创建发薪批次（New Run）

侧栏 **发薪** → **发薪批次** → **新建批次**

| 字段 | 说明 |
|---|---|
| 资金钱包 (source wallet) | 出钱方钱包；下拉只显示你创建过的钱包；链由钱包自动确定（zcash wallet = 整批走 ZEC，ethereum wallet = 整批走 USDT/USDC/ETH） |
| 周期 (pay_period) | 业务标签，例 `2026-05`，仅作为查询索引，不影响执行 |
| 备注 (notes) | 可选内部备注 |
| CSV 文件 | 员工明细，4 列：`employee_code, employee_address, amount, memo` |

**CSV 示例**（4 名员工，每人 0.5 ZEC，最后一行 memo 加密带给员工看）：

```csv
employee_code,employee_address,amount,memo
E001,u1abc...xyz,0.5,Salary 2026-05
E002,u1def...uvw,0.5,Salary 2026-05
E003,u1ghi...rst,0.5,Salary 2026-05 + bonus
E004,u1jkl...opq,0.7,Salary 2026-05 + bonus
```

**两阶段校验**：
1. **客户端校验**（上传后立即在浏览器跑）：
   - 显示预览表 N 行
   - 标错行 (缺地址 / 金额 ≤ 0 / 格式不对) 红色高亮
   - 计数显示 ✅ valid X 条 / ❌ invalid Y 条 / total
   - "创建批次"按钮只允许提交 valid 行
2. **服务端二次校验**（点确认时跑）：
   - 后端按钱包的 chain 重新校验：地址链上合法吗？金额 > 0 吗？employee_code 在花名册存在吗？
   - 任何一行不合法 → **整批 reject**（HTTP 422），返回 `validation_errors: [{row_index, field, message}]`，前端内联标红
   - 全部 valid → 创建成功，进入 `pending` 状态

> ⚠️ **为什么两阶段都要？** 客户端拦明显错误（节省网络往返），服务端是 source of truth（防 CSV 被改、防绕过前端）。

### 4.4 执行发薪（Execute Run）— 两条结果路径

进 **发薪批次** → 你刚创建的 run → **执行**

后端先匹配你配的审批策略（§4.2），按 **整批总额** vs 阈值判断走哪条路：

```
                         ┌─── 路径 A: 触发审批 ──────────────────────┐
                         │                                          │
执行按钮                 │    总额 ≥ 任一启用策略阈值                │
   │                     │    ↓                                     │
   ├──→ payroll_service ─┤    run.status → awaiting_approval        │
   │      .execute_run() │    不上链 ❄️                              │
   │                     │    返 {result:"awaiting_approval",       │
   │                     │       policy_id, threshold}              │
   │                     │    前端 1.2s flash 后自动跳 /approval/pending
   │                     │                                          │
   │                     └──────────────────────────────────────────┘
   │
   │                     ┌─── 路径 B: 直接执行 ──────────────────────┐
   │                     │                                          │
   │                     │    不命中任何策略 或 总额 < 阈值          │
   │                     │    ↓                                     │
   └─────────────────────┤    loop items → chain_client.transfer    │
                         │       逐笔上链 (per-item fan-out)         │
                         │    返 {result:"executed",                │
                         │       submitted: N, failed: M,           │
                         │       final_status:                      │
                         │         completed|partial_success|failed}│
                         │                                          │
                         └──────────────────────────────────────────┘
```

**执行结果展示**：
- **路径 A**（审批）：你不用做什么，等审批人批；审批后回 RunDetail 再次点"执行"才真上链
- **路径 B**（已执行）：立刻看 `submitted` / `failed` 计数 + 每条 item 的 tx hash + 链上确认状态

> 💡 **为什么 per-item 上链而不是一笔 tx 多 output？** M1 复用 M0 单笔转账路径（成熟稳定），每个员工一笔独立 tx，失败一个不影响其他；M2 计划上 librustzcash 单 tx 多 output Orchard，省 fee + 链上一行不暴露员工人数。

### 4.5 部分失败处理 / 取消批次

#### 单 item 重试

发薪后通常 95% 上链成功，但偶尔某员工地址输错 / 钱包余额刚好不够，会出现 partial_success。

- 在 RunDetail 表格里，failed item 红色高亮 + 显示 `error_message`（例 `invalid recipient address` / `insufficient balance`）
- 点 **重试失败条目** 按钮 → 前端逐个调 `POST /payroll/runs/{id}/items/{item_id}/retry`
- 后端只对该 item 重新跑 transfer_native，**已 confirmed 的 item 不会被重发**（按 status filter）
- 修好底层问题（充值钱包 / 改员工地址）后重试通常即可全过

#### 取消批次

`POST /payroll/runs/{id}/cancel` 可在以下状态调用：

| 状态 | 取消行为 |
|---|---|
| `pending` | 直接 DB flip 到 cancelled，没上链不影响任何东西 |
| `awaiting_approval` | 同上，相当于 maker 主动撤回 |
| `executing` 卡死 | **特殊 stuck recovery**：后端 crash 留下的卡死 run 可强清理 — 已上链 item 状态保留（不能 reverse），未提交 item 标 failed，run 标 cancelled |

> ⚠️ **铁律**：链上已 confirmed 的转账永远不能 reverse —— cancel 只是把 DB 状态归位，让你能创建新 run 继续。已花的钱要追回必须线下找收款方。

### 4.6 报表与归档

进 RunDetail 顶部点 **查看报告**：

- run 元数据：pay_period / 出账钱包 / 总金额 / 创建人 / 执行人 / 时间戳
- item 统计：submitted / failed / pending 各 N 条
- 每条 item 详情：员工 + 地址 + 金额 + 状态 + tx hash + 链上确认 + 失败原因

把这份报告导出 CSV / PDF（M2 加导出 endpoint，M1 先用浏览器打印保存 PDF）归档进每月财务底稿即可。

---

## 五、端到端业务场景演练

### 5.1 场景 A：月度发薪（最常见）

典型企业每月给 50 名员工发工资 — 总额约 25 ZEC，超审批阈值。完整流程跨 3 天：

```
Day -1 (HR 准备：发薪前 1-2 天)
─────────────────────────────────
1. 员工 → 检查花名册
   - 新入职员工已加？(单加 / CSV 补充)
   - 离职员工已 soft delete？(active=false)
   - 钱包地址变更的员工已 edit？
2. 找 IT/财务确认本月发薪用的钱包
   - 余额够吗？(打开钱包详情 → 看 shielded balance)
   - 不够 → 提前从公司主账户充值 → 等 6 个 block 确认

Day 0 (财务执行：发薪日)
─────────────────────────────────
1. 准备 CSV
   - 4 列：employee_code, employee_address, amount, memo
   - 按 HR 给的工资表填，复核金额
2. 发薪批次 → 新建批次
   - 资金钱包：选公司发薪 ZEC 钱包
   - 周期：例 "2026-05"
   - 上传 CSV → 浏览器内即时显示 50 行预览
   - 看到 ✅ 50 valid / ❌ 0 invalid → 点 创建批次
3. 后端二次校验通过 → run 进入 pending 状态
4. 点 执行 →
   ─── 总额 25 ZEC > 阈值 20 ZEC ───
   收到 `{result:"awaiting_approval", policy_id, threshold:"20"}`
   1.2 秒 flash 后自动跳到 /approval/pending
5. 微信通知审批人 (CTO / CFO 另一人)：
   "本月发薪 25 ZEC 在审批队列，劳烦看下"

Day 0 (审批人：5 分钟内)
─────────────────────────────────
1. 审批人登录 → 待审批队列 → 看到这条 run
2. 点详情 → 看金额 / 发起人 / 50 个员工列表
3. 确认无异常 → Approve (写备注："2026-05 月度工资")
4. run.status 从 awaiting_approval → approved
5. 系统通知 maker (M2 真通知，M1 maker 自己刷新页)

Day 0 (财务收尾)
─────────────────────────────────
1. 回到 RunDetail → 再次点 执行
   ─── 这次 status 已是 approved，跳过审批检查 ───
   - 后端逐个对 50 员工调 transfer_native
   - 每个 item 真链上一笔 tx
   - 返 `{result:"executed", submitted:50, failed:0, final_status:"completed"}`
2. 看 RunDetail 表格 50 条 item 全 confirmed
3. 检查 1-2 个员工微信反馈 "收到了" → 完工
4. 导出 PDF 报告归档进本月财务底稿

Day +1 (审计 trail)
─────────────────────────────────
若外部审计师在 scope 期内，他可以:
- /auditor/wallets/{id}/transfers 看到这 50 笔出账
- 申请 disclosure granularity=range 2026-05-01~2026-05-31 拿 PDF 验证总额
- 全程不需要私钥，只看链上数据
```

**关键时间点**：建议月底前 3 天完成（避免周末撞审批人不在线）。审批 SLA 默认 24h 但实操推荐 4-8h 内批完。

### 5.1.1 场景 A 应急：审批人 SLA 超时

如果审批人没看到通知，SLA 4h 后 run 自动 expired。补救：

- 进 我的审批（maker 视角）→ 看到本月发薪状态变红 expired
- **不能 reactivate 旧 run**（设计如此，避免绕过 SLA 概念）
- 重新创建 run（同 CSV 一键复用）→ 抓审批人当面批 → 执行

### 5.2 场景 B：季度审计（外部审计师 onboard 到拿到报告）

```
Day 1 (Admin 准备)
─────────────────
1. 审批策略       → 加 `wallet=财务主钱包 / zcash / ZEC / 阈值 100`
                    (大额支出审批留痕)
2. 审计师管理     → 邀请 auditor@cpa-firm.com
                  → scope = [财务主钱包, 项目A钱包]
                  → 时间窗 = 2026-01-01 ~ 2026-03-31
                  → 披露预算 = 20 次
                  → 拿 temp_password 私信给审计师
3. (可选) Export UFVK 钱包 → 私信 uview... 字符串
                    (审计师可用 Zashi 独立验证)

Day 2-30 (审计师工作)
────────────────────
1. /auditor/login + temp_password → 改强密码
2. Dashboard 看 2 个钱包基础信息
3. WalletDetail 看每个钱包真链上余额 + scope 窗内 transfers
4. 提请披露报告 ×N：
   - 单 tx granularity = 针对某笔可疑交易拿详情
   - range granularity = 整月数据生成 PDF 归档
   - 用完预算 20 次后系统会拒绝新申请
5. 把 PDF / CSV 报告归档进审计底稿

Day 30 (Admin 收尾)
──────────────────
1. 审计师管理 → deactivate 审计师账号
2. 后端再也无法登录该账号（即使旧 temp_password 也无效）
3. 已 download 的报告本地存储不受影响
```

### 5.3 场景 C：紧急转账被 SLA expire（怎么办）

```
 09:00  财务 maker 发起 50000 USDT 转账（超阈值 1000）
        → status: awaiting_approval / expiry_at: 11:00 (SLA 120min)
        → maker 通知 checker 微信"急批"

 09:00 ~ 11:00  checker 没看到通知（开会 / 出差）

 11:00  SLA worker 扫描 → flip "expired"
        → maker 在"我的审批"页看到状态变红

 11:01  财务 maker 怎么办？
        ────────────────────────
        ✅ 直接重发：/transfers → 新填同样金额 → 新 awaiting_approval 行
                                  (旧的 expired 行作为审计记录保留)
        ❌ 不能 reactivate 旧的 expired 行（设计如此，避免绕过 SLA 的概念）

 11:02  这次 maker 抓 checker 当面批 → 5 分钟内 approve → execute → confirmed
```

### 5.4 场景 D：发薪部分失败（partial_success 处理）

发薪 50 人，3 人 fail，47 人 confirmed 的真实场景：

```
点 执行 → 等 ~30 秒（per-item fan-out）
返 `{result:"executed", submitted:50, failed:3, final_status:"partial_success"}`

RunDetail 表格：
┌────┬────────────┬─────────┬───────────┬──────────────────────────────┐
│ id │ employee   │ amount  │ status    │ tx_hash / error              │
├────┼────────────┼─────────┼───────────┼──────────────────────────────┤
│ 1  │ E001 王..  │ 0.5 ZEC │ confirmed │ 0xabc...                     │
│ 2  │ E002 李..  │ 0.5 ZEC │ confirmed │ 0xdef...                     │
│ 3  │ E003 张..  │ 0.5 ZEC │ ⚠ failed  │ invalid recipient address    │
│ 4  │ E004 周..  │ 0.5 ZEC │ confirmed │ 0xghi...                     │
│ ...│ ...        │ ...     │ ...       │ ...                          │
│ 27 │ E027 韩..  │ 0.5 ZEC │ ⚠ failed  │ insufficient balance         │
│ 35 │ E035 黄..  │ 0.5 ZEC │ ⚠ failed  │ insufficient balance         │
│ ...│ ...        │ ...     │ ...       │ ...                          │
└────┴────────────┴─────────┴───────────┴──────────────────────────────┘

诊断:
- E003: 地址输错 → 找员工要新地址 → 进 员工 → edit E003 → 回 RunDetail
- E027 / E035: 钱包余额耗尽（最后几个员工时余额不够 fee + amount） → 充值钱包

修复:
1. 充值钱包 5 ZEC，等 6 block 确认
2. 改 E003 钱包地址
3. RunDetail → 点 重试 N 个失败
   - 前端 fan-out 3 个独立 retry 请求
   - 只重试 failed 行（confirmed 的不动）
4. 等 ~5 秒 → 3 笔上链 → run.status 自动从 partial_success → completed
```

**为什么不一开始就预扣余额防 insufficient？** 设计权衡：
- 预扣余额需要锁定钱包，跟其他单笔转账串行化，扩展性差
- M1 选 best-effort + retry 模式：让你看到具体哪几个失败，定向修复
- M2 会加 dry-run 预估总 fee + 余额校验，提示"余额不够 N ZEC"避免发完一半才发现

### 5.5 场景 E：发薪到 ETH 钱包（多链支持）

发薪不限于 ZEC — 如果你用 USDT 给海外员工发薪：

```
1. 员工花名册：海外员工 chain 改 ethereum，地址填 0x...
2. New Run: source wallet 选公司 USDT 钱包（chain=ethereum）
3. CSV 用 USDT 数额（例 amount=2000 表示 2000 USDT）
4. Execute → 每笔 USDT ERC20 转账走 chain_client.transfer_token
5. ETH 燃料费由 source wallet 支付（提前确认 source wallet 有少量 ETH）
```

⚠️ 链 + 币种由 source wallet 决定，**单批不能混链**（一批要么全 ZEC 要么全 USDT）。混链发薪需要拆两批 run。

---

## 六、故障排查与常见问题

### 6.1 登录类

| 症状 | 可能原因 | 解决 |
|---|---|---|
| Login → "Invalid username or password" | 密码错 | 找运维拿 `.env.secrets` 初始密码 |
| Login → "Rate limit exceeded" | 1 分钟内 ≥ 5 次失败 | 等 1 分钟再试 |
| Login → token 5min 内过期 | M1 JWT 24h 默认，但某些操作要求 fresh re-auth | 重新登录 |

### 6.2 审批类

| 症状 | 原因 | 解决 |
|---|---|---|
| 在"待审批队列"看不到自己发起的转账 | 设计如此（maker ≠ checker），自己看不到自己 | 让别的 user 当 checker |
| Approve 报 403 | 你是 maker 不能审自己 | 同上 |
| Reject 报 "需要 ≥ 5 字符理由" | 必填，避免空白拒绝无 audit | 写一个有意义的理由 |
| 转账卡 awaiting_approval | 没人批 → SLA 到自动 expire | 让 checker 主动批，或等 expire 后重发 |

### 6.3 审计师类

| 症状 | 原因 | 解决 |
|---|---|---|
| Admin sidebar 没有"审计师视图"入口 | 设计如此（双 JWT 物理隔离） | sign out → 用 auditor 账号到 /auditor/login |
| /auditor/wallets 报 401 "Invalid or expired auditor token" | Admin token 不能 access auditor 路由 | 同上 |
| 审计师 download 报告 410 Gone | 一次性 token 已被消耗 | Admin 重新 export viewing key |
| 披露申请直接 reject "budget exhausted" | 配额用完 | Admin 在审计师管理里增加 `max_disclosure_count` |
| Zashi import UFVK 失败 | 复制时多了/少了字符 | 重新 export，UFVK 是单行 `uview1...` 完整字符串 |

### 6.4 发薪类（详见章节四）

| 症状 | 原因 | 解决 |
|---|---|---|
| Execute Run 返 `awaiting_approval` | 总额触发审批策略 | 进 /approval/pending 等审批，或调阈值 |
| Run 卡 executing 不动 | 后端 crash 留下的 stuck 状态 | Cancel from executing（已成功 item 不变，未发的标 failed） |
| 单 item failed `insufficient balance` | 钱包余额不够发完全部 | 充值钱包，retry 失败的 item |

### 6.5 部署/运维类

| 症状 | 原因 | 解决 |
|---|---|---|
| Disclosure range 报 "Zcash RPC 401" | Zebra 容器重启后 cookie 轮换 | 运维重新读 cookie 写 .env + pm2 restart --update-env |
| 新部署后浏览器拉旧 JS | index.html 被中间代理缓存 | 强制刷新（Ctrl+Shift+R），nginx config 应有 `no-cache, no-store` |
| Backend 重启后 admin 密码变了 | `.env.secrets` 被误删 → 后端重新生成 | **永远不要删 .env.secrets**；如果误删，旧加密钱包永久不可解（M0 已警告） |

---

## 七、安全建议与推广要点

### 7.1 客户最关心的 5 个问题（销售/推广话术）

> 涵盖开发流程 / 多链支持 / staging 演示等 talking points。

1. **私钥安全**
   - 答：AES-256-GCM 落盘加密，导出需二次验证密码，导出全程审计留痕
   - 对比：Gnosis Safe 等需要每个签名人各自管理私钥；我们集中管理 + 双签 + 审计

2. **审计师拿不到私钥怎么验证？**
   - 答：ZEC 原生的 OVK/IVK/UFVK 是"只读密钥"，给出去仅能看不能转
   - 我们用 ZIP-316 标准字符串导出，审计师直接 import Zashi（开源钱包），完全独立验证

3. **超过审批阈值的转账如何防滥用？**
   - 答：maker ≠ checker 是数据库层硬约束，不靠 frontend RBAC；reject 必填理由 ≥ 5 字符；SLA 自动过期防卡死

4. **能审计什么颗粒度？**
   - 答：单笔交易 / 单地址全历史 / 时间段批量；输出 PDF + CSV + JSON 三格式；范围支持时间戳自动解析 block height（审计师不用懂区块链）

5. **多链支持？**
   - 答：M1 ETH（ERC20）+ ZEC（透明 + Orchard shielded），架构抽象到 `ChainClient` trait，新增链只需实现 trait 接口

6. **开发节奏与上线时间**
   - 答：M1 三大业务能力（F1.1 审计合规 / F2.1 maker-checker 双签 / F3.1 批量发薪）2026-05-16~17 一个夜班完整 ship，附 e2e 自动化烟测 11 步 34 个断言一键跑，每次 commit 不破坏 staging
   - 你的客户验收时间可控：上 staging 一天 → 浏览器真测一天 → prod 一天

7. **怎么试用？给客户做 POC？**
   - 答：staging 环境 <https://staging.example.com>，30 分钟可独立部署同款（见 [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md)）
   - 客户自己服务器上：apt 几行 + docker 几条 → letsencrypt 自动签 HTTPS → 浏览器即可演示
   - 客户数据完全留在他自己机器，不出我们门

8. **一站式 vs 拼凑工具链**
   - 答：传统做法 = 钱包（MetaMask / Zashi）+ 多签（Gnosis Safe）+ 发薪脚本（Bash / Python 写死）+ 审计导出（钱包截图 / 第三方块浏览器 + 手工 Excel）→ 工具链碎、信任面分散、审计 trail 拼接难
   - zPay Enterprise：单一登录入口 / 单一审计 trail / 单一 RBAC 模型 / 单一钱包加密层 — **你的客户原有 M0 多链钱包能力，加 3 个企业刚需，就是一站式企业级 Web3 财务平台**

9. **隐私 vs 透明的平衡**
   - 答：Zcash Orchard 默认 shielded（链上看不到收款人 / 金额 / memo），但你想给特定审计师看的时候，**通过 viewing key 自助控制** — 不给私钥、不发交易、只导出读取权限
   - 透明链（ETH 等）默认所有人可见，反而需要 zPay 这种 RBAC + 双签层挡误操作
   - 客户可以**按业务场景混用**：高敏感发薪走 ZEC + viewing key 审计；流水转账走 ETH + 双签

10. **可演示性 (sales demo path)**
    - 创建 ZEC 钱包 → 配审批策略阈值 0.01 → 录 1 个员工 → 发薪 0.05 → 触发 awaiting_approval → 在另一个浏览器窗口审批 → 执行 → 链上确认 → 拿 viewing key 给客户的"审计师"账号 → 该账号独立看到这笔交易并下载 PDF 报告 — 全程 5 分钟，覆盖三大业务能力

### 7.2 安全部署 checklist（运维交付前必做）

```
□ admin 初始密码改强密码（≥12 字符）, .env.secrets 仍备份但已不再敏感
□ 配 letsencrypt 自动续签 (certbot --nginx -d 你的域名)
□ nginx index.html 是 no-cache, /assets/ 是 immutable（避缓存事故）
□ MySQL volume 单独挂载到加密分区，定期 mysqldump 异地备份
□ Zebra mainnet RPC cookie 写到 .env 0600，仅 backend 可读
□ 防火墙：80/443 公网开放，8090(backend) / 3307(mysql) / 8232(rpc) 全 loopback only
□ 监控：pm2 logs zpay-staging-backend + grafana / promtail 接 RUST_LOG
□ 灾备：钱包 .env.secrets 备份到至少 2 个异地加密存储；丢了 = 历史钱包永久解不开
□ 业务流：审批策略至少配一条 global / 任意链 / 阈值合理（防全员发起无审批的失误）
```

### 7.3 维护节奏建议

- **每月**：核对员工花名册（新入职 + 离职）、检查审批策略覆盖范围
- **每季度**：审计师 scope 时间窗续期 / deactivate 离职 cycle
- **半年**：letsencrypt 证书自动续 + 自行回归测一遍 ./e2e/smoke.sh 34/34
- **年度**：审计师管理凭证轮换，所有 active auditor 走一次 reset password

---

## 八、Ironwood 迁移与批量隐私转账（F4）

> 读者：财务负责人 / 资金操作员。两个功能都会动金库资金，均需 **admin** 角色。

### 8.1 为什么钱包页出现迁移提示横幅

Zcash 的 **NU6.3** 网络升级（主网 2026-07-28 激活）启用名为 **Ironwood** 的新屏蔽池（验证机制被钉死/pinned），同时旧 Orchard 池关闭存入和池内转账。资金本身始终安全：旧池的每一笔资金都可以通过单向的 **turnstile（过闸）** 迁入 Ironwood，只出不进。（新电路的独立审计与外部形式化验证仍在进行中——"钉死"不等于"已形式化验证"。）

对金库意味着什么：

- **迁移前**：屏蔽余额照常显示、照常安全，但激活后旧池的屏蔽转账进出会停摆。
- **迁移后**：一切照旧——地址不变、密钥不变、流程不变，只是底层池子换了。
- 每个迁移批次只付正常网络手续费，没有其他损耗。

### 8.2 迁移一个钱包（三步向导）

1. 打开 Zcash 钱包页，仍持有旧池资金的钱包会出现提示横幅 → 点 **迁移**。
2. **第 1 步·说明**：迁移做什么，确认钱包。
3. **第 2 步·模式**：
   - **私密（推荐）**——把余额拆成若干**随机大小**的批次，在时间窗内错峰执行（默认 6 批 / 48 小时），避免在过闸处留下"某公司 14:02 整体搬家"的明显指纹。批次金额刻意随机化，就是防止等额切块互相关联；注意过闸是全流程中唯一暴露金额的环节，这里的"私密"是**降低可关联性**，不是保证完全不可关联。
   - **立即**——单批马上走。适合小余额或时间优先的场景。
4. **第 3 步·确认**：核对拆分计划（每批金额+时间表）后创建。
5. 点 **立即执行**（立即模式）或 **启动调度**（私密模式）。总额触发审批策略时，任务转入*待审批*——需**另一位管理员**批准（发起人不能批自己的单，这是数据库层约束，不只是界面限制）。**一次批准覆盖整个时间窗**，之后批次无人值守自动执行。
6. 在进度页跟踪：每批状态、交易哈希、失败原因原文。**失败批次不阻塞其他批次**，可单独一键重试。
7. **取消**是停止剩余批次的唯一方式；已上链的批次无法撤回。

服务重启（升级、宕机）无影响：调度表存在数据库里，重启后从断点继续。

### 8.3 批量隐私转账（任意收款人）

把批量发薪泛化到**任意收款人列表**——供应商、返佣、发放——以屏蔽 ZEC 支付，并可选隐私调度。

1. 侧边栏 → **批量隐私转账** → **新建**。
2. 填标题、选付款钱包，上传 CSV，列为：

   ```
   recipient_address, amount, memo
   utest1abc...,      1.25,   invoice-001
   ```

   - 表头行可选；memo 选填（≤ 512 字节）。
   - **收款地址必须是支持 Orchard 的统一地址**（`u1...`）。透明地址 / 仅 Sapling 地址会被拒——本功能设计上只走屏蔽转账；透明打款请用普通转账页。
   - 服务端逐行校验并**一次性返回全部错误**（地址不合法、金额非正、完全重复行、总额超余额），并映射回 CSV 行号。一遍修完重新上传即可。
3. **隐私调度**：
   - **关闭**——全部立即排队。
   - **错峰**——转账打乱后分 N 批在时间窗内错开（默认 4 批 / 24 小时），避免付款时间把整批关联起来。可选**单笔上限**：超过上限的行自动拆成多笔随机金额的小额转账。
4. 审批、执行、进度、单笔重试、取消与迁移完全一致（§8.2 第 5–7 条）。金额**绝不静默缩水**：余额不够时该笔直接失败并保留节点原文错误，补足余额后重试。

### 8.4 关于「屏蔽」的两件反直觉的事

两条都是协议行为，不是产品缺陷。NU6.3 之后屏蔽会把新资金送进 Ironwood，届时生效。

**1. 屏蔽是按币整枚花的，余额会以屏蔽找零的形式回到自己钱包。**
你想屏蔽 2 ZEC，而透明余额是一枚 3.125 ZEC 的币，那么这枚币会被整枚花掉：
2 ZEC 到你指定的地方，剩下约 1.125 ZEC **作为第二笔屏蔽 note 回到你自己的钱包**。
钱一分没少，只是从透明变成了屏蔽。
反过来做（把找零退回透明地址）等于在链上公开「这个地址刚花了一笔、还剩多少」，
屏蔽的意义会被削掉一半。

**2. 屏蔽手续费按「花掉几枚币」算，不按金额算。**
1 枚币 15,000 zatoshi；同样的总额如果由 10 枚零散的币组成，就是 60,000 zatoshi。
这是 ZIP-317 的规则（每个透明输入算一个 logical action），所以**靠很多笔小额充值攒起来的钱包，
屏蔽成本明显高于一笔大额充值的钱包**。
如果余额不足以覆盖所选币的手续费，系统会在事前直接拒绝并同时给出两个数字，
而不是少付手续费、让交易被网络拒掉。

### 8.5 常见问题

| 现象 | 原因 | 处理 |
|---|---|---|
| 屏蔽的比我要求的多 | 屏蔽按币整枚花（§8.4） | 正常——多出来的部分已作为屏蔽 note 回到你自己钱包 |
| 同样金额，我的屏蔽手续费比别人高 | 手续费按花掉的币的枚数算（§8.4） | 正常——在意手续费可预期性的话，先归集零散充值 |
| 横幅提示旧池资金但余额看起来正常 | 正常——横幅反映资金在哪个池，不代表有风险 | 在下次需要屏蔽转账前完成迁移即可 |
| "wallet already has migration run #N ..." | 每个钱包同时只允许一个进行中的迁移 | 先完成或取消当前迁移 |
| 执行后变成"待审批" | 总额触发了审批策略 | 由另一位管理员批准（发起人自批返回 403） |
| CSV 被拒并列出行错误 | 地址/金额/重复行问题 | 错误一次全给，修完文件重新上传 |
| 某批次/某笔显示失败 | 节点拒绝或执行时余额不足 | 查看保存的错误原文（即节点原话），排除原因后点重试 |
| 时间窗内后端重启过 | 无影响 | 调度是数据库驱动的，剩余批次照表执行 |

---

(本文档持续维护)

**最后更新**：2026-07-24

**文档反馈**：请通过仓库 issue 提交
