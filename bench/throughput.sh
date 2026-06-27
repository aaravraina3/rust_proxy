#!/usr/bin/env bash
# Throughput benchmark: requests/sec sustained through the proxy vs straight to
# the upstream, using ApacheBench with keep-alive (-k). Uses the bundled
# keep-alive upstream binary so the upstream isn't the bottleneck. Starts both,
# runs ab, then tears everything down.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

N="${1:-50000}"   # total requests
C="${2:-100}"     # concurrent connections

cargo build --release

./target/release/upstream 127.0.0.1:8080 >/tmp/upstream.log 2>&1 &
UP=$!
RUST_LOG=error ./target/release/rust_proxy --listen 127.0.0.1:9000 --target 127.0.0.1:8080 >/tmp/proxy.log 2>&1 &
PX=$!

cleanup() { kill "$UP" "$PX" 2>/dev/null || true; }
trap cleanup EXIT

sleep 2

echo "=== direct upstream :8080 ==="
ab -k -n "$N" -c "$C" http://127.0.0.1:8080/ 2>/dev/null | grep -E "Requests per second|Failed requests"
echo "=== through proxy :9000 ==="
ab -k -n "$N" -c "$C" http://127.0.0.1:9000/ 2>/dev/null | grep -E "Requests per second|Failed requests"
