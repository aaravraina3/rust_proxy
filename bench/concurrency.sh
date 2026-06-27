#!/usr/bin/env bash
# Concurrency sweep: how many simultaneous keep-alive connections the proxy
# sustains, and the req/sec at each level. Starts the bundled upstream and the
# proxy, runs ab at rising concurrency, then tears everything down.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

ulimit -n 65536 || true   # need enough fds for thousands of sockets
N="${1:-100000}"          # requests per level

cargo build --release

./target/release/upstream 127.0.0.1:8080 >/tmp/upstream.log 2>&1 &
UP=$!
RUST_LOG=error ./target/release/rust_proxy --listen 127.0.0.1:9000 --target 127.0.0.1:8080 >/tmp/proxy.log 2>&1 &
PX=$!

cleanup() { kill "$UP" "$PX" 2>/dev/null || true; }
trap cleanup EXIT

sleep 2

echo "concurrency sweep through proxy (keep-alive):"
for C in 200 1000 2000 4000; do
  out=$(ab -k -n "$N" -c "$C" http://127.0.0.1:9000/ 2>/dev/null \
        | grep -E "Failed requests|Requests per second")
  rps=$(echo "$out" | awk '/Requests per second/ {print $4}')
  fail=$(echo "$out" | awk '/Failed requests/ {print $3}')
  echo "  concurrency=$C  ->  ${rps:-FAILED} req/sec, failed=${fail:-?}"
done
