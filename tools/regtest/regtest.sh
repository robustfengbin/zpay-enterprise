#!/usr/bin/env bash
# zebra regtest 跑道 —— F4 (Ironwood 迁移 / 批量隐私转账) 的本地测试链
#
# 为什么需要它: 公共 testnet 的 NU6.3 早已激活 (4,134,000)，旧 Orchard 池被
# turnstile 封成"只出不进"，没法往旧池注资，迁移这条核心路径的测试起点根本搭不
# 起来。自建链可以自己定激活高度、秒级出块、反复重放。
#
#   ./regtest.sh start A     # 档A: NU5..NU6.2 全在 1，无 NU6.3 (旧池永久活跃)
#   ./regtest.sh start B     # 档B: 同上 + NU6.3 @ 500 (跨激活过闸测试)
#   ./regtest.sh mine  B 505 # 出 505 个块 (跨过 NU6.3)
#   ./regtest.sh info  B     # 高度 + 各 NU 激活状态
#   ./regtest.sh stop  all
#
# 状态/日志落在 run/ (gitignore)，所以换机器只要有 zebrad 就能重建。
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUN_DIR="$HERE/run"
ZEBRAD="${ZEBRAD:-$HOME/prj/zebra-latest/target/debug/zebrad}"

rpc_port() {
  case "$1" in
    A) echo 28232 ;;
    B) echo 28233 ;;
    *) echo "档只能是 A 或 B" >&2; return 1 ;;
  esac
}

render_config() {
  local tag="$1" state_dir="$RUN_DIR/state-$tag" out="$RUN_DIR/config-$tag.toml"
  mkdir -p "$state_dir"
  # zebra 的 [state] cache_dir 要绝对路径，所以配置是模板 + 渲染，而不是把某台
  # 机器的路径写死进仓库。
  sed "s|@STATE_DIR@|$state_dir|" "$HERE/config-$tag.toml.template" > "$out"
  echo "$out"
}

start_one() {
  local tag="$1" port config
  port="$(rpc_port "$tag")" || exit 1
  [ -x "$ZEBRAD" ] || { echo "找不到 zebrad: $ZEBRAD (可用 ZEBRAD=... 覆盖)"; exit 1; }
  mkdir -p "$RUN_DIR"

  if [ -f "$RUN_DIR/pid-$tag.txt" ] && kill -0 "$(cat "$RUN_DIR/pid-$tag.txt")" 2>/dev/null; then
    echo "档$tag 已在运行 pid=$(cat "$RUN_DIR/pid-$tag.txt") — RPC http://127.0.0.1:$port"
    return 0
  fi

  config="$(render_config "$tag")"
  nohup "$ZEBRAD" -c "$config" start > "$RUN_DIR/log-$tag.txt" 2>&1 &
  echo $! > "$RUN_DIR/pid-$tag.txt"
  echo "档$tag 启动 pid=$(cat "$RUN_DIR/pid-$tag.txt") — RPC http://127.0.0.1:$port"
  echo "  日志: $RUN_DIR/log-$tag.txt"
  for _ in $(seq 1 30); do
    grep -q "Opened RPC" "$RUN_DIR/log-$tag.txt" 2>/dev/null && { echo "  RPC ready."; return 0; }
    sleep 1
  done
  echo "  (警告: 30s 内未见 'Opened RPC'，检查日志)"
}

stop_one() {
  local tag="$1" pf="$RUN_DIR/pid-$tag.txt" pid
  [ -f "$pf" ] || { echo "档$tag: 无 pid 文件，可能未启动"; return; }
  pid="$(cat "$pf")"
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid"; sleep 2; kill -9 "$pid" 2>/dev/null
    echo "档$tag: 已停止 pid=$pid"
  else
    echo "档$tag: pid=$pid 不在运行"
  fi
  rm -f "$pf"
}

rpc() {
  local port="$1" method="$2" params="$3"
  curl -s --max-time 1800 -H 'content-type:application/json' \
    --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$method\",\"params\":$params}" \
    "http://127.0.0.1:$port/"
}

case "${1:-}" in
  start)
    case "${2:-all}" in
      A|B) start_one "$2" ;;
      all) start_one A; start_one B ;;
      *) echo "用法: $0 start [A|B|all]"; exit 1 ;;
    esac
    ;;
  stop)
    case "${2:-all}" in
      A|B) stop_one "$2" ;;
      all) stop_one A; stop_one B ;;
      *) echo "用法: $0 stop [A|B|all]"; exit 1 ;;
    esac
    ;;
  mine)
    tag="${2:?用法: $0 mine <A|B> <N>}"; n="${3:?用法: $0 mine <A|B> <N>}"
    port="$(rpc_port "$tag")" || exit 1
    echo "档$tag (port $port) 出 $n 个块 ..."
    # regtest 关了 PoW，generate 内部走 getblocktemplate + submitblock。
    rpc "$port" generate "[$n]" | python3 -c \
      "import sys,json; r=json.load(sys.stdin); h=r.get('result'); print('生成块数:', len(h) if h else 0, '| err:', r.get('error'))"
    rpc "$port" getblockchaininfo '[]' | python3 -c \
      "import sys,json; print('当前高度:', json.load(sys.stdin)['result']['blocks'])"
    ;;
  info)
    tag="${2:?用法: $0 info <A|B>}"
    port="$(rpc_port "$tag")" || exit 1
    rpc "$port" getblockchaininfo '[]' | python3 -c \
      "import sys,json; d=json.load(sys.stdin)['result']; print('chain:',d['chain'],'blocks:',d['blocks']); [print(' ',v.get('name'),v.get('status'),'@',v.get('activationheight')) for v in d.get('upgrades',{}).values()]"
    ;;
  *)
    echo "用法: $0 {start|stop} [A|B|all] | $0 mine <A|B> <N> | $0 info <A|B>"
    exit 1
    ;;
esac
