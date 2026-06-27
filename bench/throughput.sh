#!/usr/bin/env bash
# Throughput benchmark: requests/sec sustained through the proxy vs straight to
# the upstream, using ApacheBench. Starts a local upstream and the proxy,
# fires N requests at concurrency C, then tears everything down.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

N="${1:-20000}"   # total requests
C="${2:-100}"     # concurrent connections

cargo build --release

python3 -m http.server 8080 --bind 127.0.0.1 >/tmp/upstream.log 2>&1 &
UP=$!
./target/release/rust_proxy --listen 127.0.0.1:9000 --target 127.0.0.1:8080 >/tmp/proxy.log 2>&1 &
PX=$!

cleanup() { kill "$UP" "$PX" 2>/dev/null || true; }
trap cleanup EXIT

sleep 2

echo "=== direct upstream :8080 ==="
ab -n "$N" -c "$C" http://127.0.0.1:8080/ 2>/dev/null | grep -E "Requests per second|Time per request|Failed requests"
echo "=== through proxy :9000 ==="
ab -n "$N" -c "$C" http://127.0.0.1:9000/ 2>/dev/null | grep -E "Requests per second|Time per request|Failed requests"
