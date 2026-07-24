# F4 剩余开发工作需求文档(后续 session 按此施工)

> 作者:jiaxu 🐍 · 2026-07-24 晚
> 用途:**后续任何 session(jiaxu / luxun)开工前先读本文**,按工作项领活;
> 背景与产品需求见同目录 [PRD-F4-batch-privacy-transfer.md](./PRD-F4-batch-privacy-transfer.md)(定稿版,含 luxun 4 条修订)。
> 分工铁律:**luxun = F4.0 底层(orchard 内核/Cargo.toml/network)+ regtest 跑道;jiaxu = F4.1/F4.2 引擎+前端+e2e**。共享文件只追加行;别双改同一文件。

---

## §0 现状锚点(2026-07-24 22:00 UTC+8 时点)

| 项 | 状态 |
|---|---|
| 分支 `feat/f4.0-zcash-stack-upgrade` | luxun own。`6c4c658` = F4.0-a 达标(orchard 0.13→0.15.3 编译绿,回归 26绿/4红 = main 基线,jiaxu 独立复验通过) |
| 分支 `feat/f4.1-migration-engine` | jiaxu own。`1fe1bd5` = F4.1 后端(`ab8143b`)+ 前端(`e4d5bd7`)+ **已 merge F4.0 分支**(零冲突)。33绿/4红 |
| 4 个基线红测 | main 就有的 pre-existing(`sync_config_default` 25≠10 / `address_to_script_pubkey_t1` / encryption key 32 字节 ×2),**不属于 F4 范围,谁修都单独 commit,别混入功能 PR** |
| regtest 档 A | luxun 起,常驻 `http://127.0.0.1:28232`,NU5..NU6.2@1、无 NU6.3、旧池永久活,`generate`/`generatetoaddress` 可出块,当前高度 ~116 |
| regtest 档 B | NU6.3@500,**未常驻**,等 W2 做完由 luxun 起 |
| jiaxu e2e 环境 | docker `zpay-e2e-mysql-jiaxu`(127.0.0.1:33390 / root / e2e_root_pw_2026 / 库 zpay_e2e)+ backend 127.0.0.1:8080(`backend/.env` 已配,gitignored);e2e 脚本在 jiaxu session scratchpad `e2e-f41-migration.sh`(W3 时应搬进 repo `backend/scripts/`) |
| PRD 定稿分支 | `docs/2026-07-24-f4-batch-privacy-transfer`(`6a57a54`);本文档随 F4.1 分支走 |

---

## §1 工作项总表(按依赖顺序)

| # | 工作项 | Owner | 依赖 | 状态 |
|---|---|---|---|---|
| W1 | F4.0-b network-awareness(解 e2e 两阻塞) | luxun | 无 | **下一个,最急** |
| W2 | F4.0-c 双池核心改造(height-aware builder + 双树) | luxun | W1 | 未开工 |
| W3 | F4.1 完整 e2e(档 A)+ 前端浏览器实测 review | jiaxu | W1 | 脚本已备,等 W1 |
| W4 | 档 B turnstile 跨激活 e2e | luxun 起档B + jiaxu 跑 | W2 | 未开工 |
| W5 | F4.2 批量隐私转账执行层 + 前端 | jiaxu | W2 merge | 表已建,余未开工 |
| W6 | USER-MANUAL 双语章节 + e2e smoke 脚本入库 | jiaxu | W3 | 未开工 |
| W7 | PR 合入 main(先 F4.0 后 F4.1,交叉 review) | 双方 | W3(最低)/ W4(完整) | 未开工 |
| W8 | 部署上线 | **待 Robust 拍**(见 §9) | W7 | — |

---

## §2 W1 — F4.0-b network-awareness(luxun)

**问题**(jiaxu e2e 实测挖出 + luxun 侦察补全,已定位):
1. 地址编码写死主网:taddr 恒 `t1...`、UA 恒 `u1...`,regtest 的 `generatetoaddress` 拒收("Address is for Main but we expected Regtest"),资金进不了钱包。位置:`orchard/address.rs`(t1 前缀 / `ua.encode(Main)` / `!=Main` 只 warn 不换网)+ `crypto/zcash.rs`(t1/u1 判断)+ keys 的 HRP 选择
2. 主网 NU5 激活高度 `1_687_104` 硬编码 **~15 处**(luxun 全量侦察,行号在他手里):`services/wallet_service.rs:108/437/578/735/881`、`orchard/witness_sync.rs:748-750/1052`、sync/db 等。短链上 sync 报 "not in the main chain"(:578 的 `max(1_687_104)` 是直接死因)。**改造锚点:`transfer.rs:206` 已有 NetworkType match,扩上 regtest 分支后作为统一入口**

**已定设计**(群里对齐过):
- network 来源 = `getblockchaininfo.chain` 字段(`main`/`test`/`regtest`),**不新增配置项**
- ⚠️ 暗雷:`update_rpc` 支持运行时切节点 → **network 不能启动缓存到死**;绑 ZcashClient 实例、update_rpc 时强制重读,派生地址惰性取当前值
- HRP 速查:regtest = taddr `tm` / UA `uregtest` / Sapling `zregtest`;testnet = `tm` / `utest` / `ztestsapling`
- 激活高度**统一从链上读**:`getblockchaininfo.upgrades[NU5].activationheight`(luxun 定案,一个来源覆盖 main/test/regtest,连 mainnet 1,687,104 都不写死,~15 处硬编码连根拔);三套常量(mainnet 1,687,104 / testnet 1,842,420 / regtest 按节点配置)只作 RPC 不可用时的 fallback。⚠️ 与 network 字段同款缓存纪律:绑 ZcashClient、update_rpc 时重读

**验收**:
- [ ] 连档 A 建 zcash 钱包 → taddr 是 `tm...`,UA 是 `uregtest...`
- [ ] `generatetoaddress [101, <tm地址>]` 成功
- [ ] `/zcash/scan/sync` 在 <200 高的 regtest 链上不再报 "not in main chain"
- [ ] 连主网节点行为不变(回归 26 绿不少)
- [ ] wallet_service.rs 借道的行改完通知 jiaxu(避免同文件冲突)

---

## §3 W2 — F4.0-c 双池核心改造(luxun)

PRD §3.2 全文适用,要点 + review 已留的账:

1. **version 接缝动态化**:现 transfer.rs 四处硬填 `orchard_v2/FixedPostNu6_2/NoteV2/TxV5`(每处有"双池接缝"注释)。收敛为单一 helper `bundle_version_for(branch, pool)`(luxun 定名),按 (链高度 vs NU6.3 激活高度, 目标池) 选:
   - 旧池花费 @ NU6.3 前 = `orchard_v2` + TxV5(现状)
   - 旧池花费 @ NU6.3 后(turnstile 过闸)= `BundleVersion::orchard_v3()` 花费 + 输出到 **Ironwood**(`ironwood_v3`),v6 交易
   - 纯新池转账 = `ironwood_v3` + TxV6
2. **欠账必还**(F4.0-a review watch item):`compute_orchard_digest` / `compute_orchard_digest_from_proven` 改收 `TxVersion` 参数,消灭 `.expect("Orchard pool V5 commitment is always valid")`——Ironwood bundle 出现后这是生产 panic
3. **ProvingKey**:`FixedPostNu6_2` 与 `PostNu6_3` 两把,OnceLock 改双缓存(或 map)
4. **DB**:`orchard_notes` 加 `pool` 列(`orchard`/`ironwood`),migration 带历史回填(存量全 `orchard`);扫描落库按 bundle 版本写对 pool
5. **scanner/tree/witness 双池**:v5+v6 bundle 都解析;两棵 commitment tree 分开跟踪(旧池树激活后冻结只服务旧 note witness,新池树增长)。**注意 NoteVersion::V3 (rcm_v3) 的 trial-decrypt 路径**
6. **v6 交易序列化**:检查 transaction.rs 的 tx 构造(现 `TX_VERSION=0x80000005`/`VERSION_GROUP_ID_V5` 硬编码)——v6 交易格式、digest personalization、anchor 位置都不同,这是 W2 里最容易漏的面
7. **F4.1 对接零改动确认**:jiaxu 的 executor 走 `wallet_service.create_privacy_transfer_proposal → execute_privacy_transfer` 自地址转账;W2 做完后此路径在激活后应自动构造过闸交易——**语义务必保持:金额、fund_source=Shielded、to=自己 UA 不变,变的只是底层 bundle**

**验收**:
- [ ] 档 A(NU6.2 语义)回归:单笔 shielded 转账闭环(发出→扫回→再花)
- [ ] 档 B(NU6.3@500):激活前注资旧池 → 跨激活 → 旧池 note 过闸进 Ironwood 成功上链 → 新池内转账成功 → 新池 note 再花成功
- [ ] 旧池在激活后拒绝池内转账(消费端报错清晰,PRD F4.1.9 的后端拒绝面)
- [ ] 26 绿基线不掉

---

## §4 W3 — F4.1 完整 e2e + 前端实测(jiaxu,W1 后立即)

1. merge 最新 F4.0 分支进 F4.1 分支(历史:`1fe1bd5` 已并过一次,零冲突)
2. 跑 e2e 脚本全程(脚本在 scratchpad,**本步顺手提交到 `backend/scripts/e2e-f41-migration.sh`**):
   - mine 101 块到钱包 tm 地址 → shield 3 ZEC 到自己 UA → 确认 scan 到 note
   - `private` 模式 3 批/1h 窗口:创建→execute→首批立即、余批按表
   - **审批腿**:建低阈值 policy → 触发 awaiting_approval → maker 自批必须 403 → 第二个 admin 批准 → 批次自动继续(approve 覆盖整窗口语义)
   - **断点续跑**:批 2、3 之间 kill backend → 重启 → 余批自动继续(全 DB 驱动)
   - **cancel 腿**:第二个 run 中途 cancel → pending 批次停、已上链的不动
   - `immediate` 模式单批扫尾
3. **前端浏览器实测 review**(Robust 铁律:UI 改必测试):`npm run dev` 起前端,真点一遍横幅→向导→进度页→审批→取消;截图发群;发现的 UI 问题当场修
4. 验收:全部腿绿 + 截图 + 群里汇报

---

## §5 W4 — 档 B turnstile e2e(W2 后,双方)

- luxun 起档 B 常驻(NU6.3@500)并把 regtest 基建(config/脚本)固化进 repo(他已承诺)
- jiaxu 在档 B 复跑 W3 全套,重点新增:**激活前注资旧池 → 跨激活 → 迁移 run 真过闸**;验证迁移完成后旧池余额=0(减手续费)、新池到账、审计导出(F1.1)标注正确
- 这是 PRD §8 说的"唯一能受控覆盖旧池花费→turnstile 路径"的测试,W7 合 main 前必须绿

---

## §6 W5 — F4.2 批量隐私转账执行层 + 前端(jiaxu,W2 merge 后)

表(`batch_transfer_runs/items`)W1 阶段已建好定型。剩:
1. **执行层**:repo + service 复用 F4.1 模式;executor 扩展为双 run 类型(migration = 自地址;batch_transfer = 任意收款人,按 item.recipient_address 走同一 proposal→execute 路径);隐私调度参数 `privacy_mode: off|staggered`(分批数/时间窗/单笔上限拆分)
2. **校验**:CSV/JSON 导入两级校验(地址合法性**按新池规则**、金额>0、总额≤余额、去重)——复用 F3.1 的两级校验骨架
3. **API**:`/batch-transfers` 系列,镜像 `/migrations` 8 端点
4. **前端**:批量转账页(CSV 上传+校验结果表+隐私调度参数区),交互骨架抄 Payroll RunCreate/RunDetail
5. 注意 PRD D2 方案 A:**不动 payroll**,P2 再迁
6. 验收:档 A/B 上 N 收款人批量真跑 + partial_failure 重试腿 + UI 实测

---

## §7 W6 — 文档与脚本入库(jiaxu,W3 后)

- USER-MANUAL-EN/CN 补"Ironwood 迁移"章节(按角色写:财务负责人视角;**表述纪律:不写"已形式化验证",只写"验证机制被钉死+独立审计与 FV 进行中";不承诺"完全不可关联"**)
- e2e 脚本入库 `backend/scripts/`;11-step smoke 扩展两段(PRD §8)

---

## §8 W7 — 合入 main(双方交叉 review)

1. **先 F4.0 PR**(luxun 发,jiaxu review):W1+W2 完整、档 A/B 回归绿。review 重点:双池接缝无硬编码残留、`.expect` 清零、v6 序列化
2. **后 F4.1 PR**(jiaxu 发,luxun review):把 F4.1 分支 rebase/merge 到含 F4.0 的 main
3. 配置文件改动单独 commit;**禁 push -f**;push 前 `git diff --stat`
4. GitHub 开源镜像同步:main 合完由 Robust/惯例流程推,保持两仓一致(07-24 核实过零分叉,别破)

---

## §9 开放问题(需 Robust 拍板)

| # | 问题 | 背景 |
|---|---|---|
| Q1 | **部署归属与时点**:zpay 部署上线谁做、部署到哪?(本机有一套 `zpay-staging-mysql` 在跑,归属待确认;按惯例部署归江儬,但 zpay 此前不在江儬管辖清单里) | W8 前置 |
| Q2 | 7/28 当天主网节点(zebra-mainnet 容器已在本机)是否要提前升到 v6.2.1?旧版本节点激活日会掉链 | 运维项 |
| Q3 | F4.2 与 USER-MANUAL 的优先级顺序(现按 W5→W6 排,若想先文档后批量可调) | 排期 |

---

## §10 环境速查(后续 session 直接用)

```
档A regtest RPC   http://127.0.0.1:28232   (luxun 起,generate/generatetoaddress 可用)
e2e MySQL        127.0.0.1:33390  root / e2e_root_pw_2026 / zpay_e2e  (docker: zpay-e2e-mysql-jiaxu)
e2e backend      127.0.0.1:8080   (zpay-enterprise/backend/.env 已配;跑法: ./target/debug/web3_wallet_service)
e2e admin 登录    admin / jiaxu-e2e-admin-password-2026!
前端 dev         cd frontend && npm run dev  (Vite, ALLOWED_ORIGIN 已配 localhost:5173)
```

工程铁律重申:**全 debug 编译**、禁 mock/fallback、禁 scp 部署、PM2 delete+start、UI 改必实测、DB 操作先备份、失败原文落库不吞。
