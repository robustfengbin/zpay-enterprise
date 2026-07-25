# zebra regtest 跑道（F4 Ironwood 迁移 / 批量隐私转账测试）

## 为什么不用公共 testnet

公共 testnet 的 **NU6.3 早就激活了**（testnet 4,134,000；主网 3,428,143 = 2026-07-28）。
NU6.3 一激活，旧 Orchard 池就被 turnstile 封成「只出不进」——**没法再往旧池注资**，
而「旧池有钱 → 跨激活 → 过闸进 Ironwood」正是要验的那条路径，测试的起点就搭不起来。

自建链还顺带解决三件事：出块我说了算（`generate N` 秒出，不等 75 秒一块）、激活高度
我说了算（反复演激活前后的行为切换）、错一笔清库重来（公共链上错了永久留痕）。

## 两条跑道

| 档 | 激活高度 | RPC | P2P | 用途 |
|----|----------|-----|-----|------|
| A | NU5/NU6/NU6.1/NU6.2 @ 1，**无 NU6.3** | 127.0.0.1:28232 | 28244 | 旧池单笔 + 批量全路径回归（旧池永久活跃） |
| B | 同上 + **NU6.3 @ 500** | 127.0.0.1:28233 | 28245 | 跨激活过闸（旧池 → Ironwood）e2e |

端口选 28xxx 是因为本机 18232/18244 被占。`enable_cookie_auth = false`，裸调 RPC 不用带凭据。

## 用法

```bash
./regtest.sh start A          # 起档A
./regtest.sh start B          # 起档B
./regtest.sh mine  B 505      # 出 505 块（跨过 NU6.3@500）
./regtest.sh info  B          # 高度 + 各 NU 激活状态
./regtest.sh stop  all
```

`zebrad` 默认取 `~/prj/zebra-latest/target/debug/zebrad`，换机器用 `ZEBRAD=/path/to/zebrad ./regtest.sh …` 覆盖。
配置是 `config-{A,B}.toml.template` + 启动时渲染（zebra 的 `[state] cache_dir` 要绝对路径，
所以不把某台机器的路径写死进仓库）。状态、日志、pid、渲染后的配置全在 `run/`，已 gitignore ——
**换机器只要有 zebrad 就能重建整条跑道**。

## 踩过的坑 / 必须知道的事实

- **`getblockchaininfo.chain` 回的是 `"test"`，不是 `"regtest"`**。这两条链本质是
  testnet + 自定义激活高度的形态。所以本代码生成的是 **testnet 系地址**：t-addr `tm…`、
  UA `utest1…`（**不是** `uregtest1…`）。e2e 断言按 `tm`/`utest` 写。
- 矿工地址 `tmJymvcUCn1ctbghvTJpXBwHiMEB8P6wxNV`（testnet 系，与上面一致）。
- `disable_pow` 生效，出块靠 `generate` RPC，不需要外部矿工。
- 每笔交易 build 会重造 Orchard proving key（~20s），批量测试会累加 —— 是慢，不是错。
- 档B 上过闸后 `z_gettreestate` 会多出 `ironwood` 字段（NU6.3 起才有），
  `tx.ironwood.actions` 与 `tx.orchard.actions` 结构相同但属于**两棵独立的 commitment tree**。
  双池 scanner 的树单测就是拿这两棵树的真实 root 钉的（见 `orchard/tree.rs` 测试）。
