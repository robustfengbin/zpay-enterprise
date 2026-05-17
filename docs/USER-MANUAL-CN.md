# zPay Enterprise 用户操作手册

> 面向企业财务、合规、审计角色的端到端操作指南。
> 配套 staging 环境: <https://zpaystage.fastaitop.com>
> 技术细节请见 [PRD 文档](.) 与 [STAGING-DEPLOYMENT.md](STAGING-DEPLOYMENT.md)。
> English version: [USER-MANUAL-EN.md](USER-MANUAL-EN.md)

---

## 目录

- [一、文档导览与角色定义](#一文档导览与角色定义)
- [二、首次部署后必做](#二首次部署后必做)
- [三、审计与合规](#三审计与合规)
- [四、员工与批量发薪](#四员工与批量发薪)（sweden 章节，待补）
- [五、端到端业务场景演练](#五端到端业务场景演练)
- [六、故障排查与常见问题](#六故障排查与常见问题)
- [七、安全建议与推广要点](#七安全建议与推广要点)

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

部署完成后，访问 <https://zpaystage.fastaitop.com>（或你的部署域名），登录页：

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

1. 访问 <https://zpaystage.fastaitop.com/auditor/login>（**注意 URL 不一样**，独立入口）
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

> 本章节由 sweden 主笔，待补 — 涵盖：
> - 员工花名册管理（单加 / CSV 导入 / soft delete）
> - 批量发薪流程（New Run → 两阶段 validate → Execute → tagged union outcome）
> - F2.1 阈值 hook 在发薪场景的串通（按 run 总额触发审批）
> - 部分失败 retry 单 item + cancel from any state
> - 完整演示路径

---

## 五、端到端业务场景演练

### 5.1 场景 A：月度发薪（最常见）

> sweden 主笔，待补

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

### 5.4 场景 D：发薪部分失败（业务场景见 sweden 章节五）

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

### 6.4 发薪类（详见 sweden 章节）

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

> 联合补充章节，待 sweden 增补对开发流程 / 多链支持 / staging 演示等 talking points

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

---

(章节 4 / 5.1 / 5.4 / 7 部分由 sweden 主笔补，本文档持续维护)

**最后更新**：2026-05-17 by france 🥖 + sweden 👑

**文档反馈**：请通过 GitLab issue / Discord 团队群提交
