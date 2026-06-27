#!/usr/bin/env bash
# Self-contained latency benchmark for the proxy.
# Builds release, starts a python upstream on :8080, starts the proxy on :9000
# pointing at it, runs the latency probe, then tears everything down.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

cargo build --release

python3 -m http.server 8080 --bind 127.0.0.1 >/tmp/upstream.log 2>&1 &
UP=$!
./target/release/rust_proxy --listen 127.0.0.1:9000 --target 127.0.0.1:8080 >/tmp/proxy.log 2>&1 &
PX=$!

cleanup() { kill "$UP" "$PX" 2>/dev/null || true; }
trap cleanup EXIT

sleep 2
python3 bench/latency.py "${1:-300}"
