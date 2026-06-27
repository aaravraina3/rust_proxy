#!/usr/bin/env python3
"""Measure per-request latency through the proxy vs straight to the upstream.

Assumes an upstream HTTP server is running on :8080 and the proxy on :9000.
See run_bench.sh which wires both up for you.
"""
import socket
import statistics
import sys
import time


def bench(port, n=300):
    lat = []
    req = b"GET / HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    for _ in range(n):
        t0 = time.perf_counter()
        s = socket.create_connection(("127.0.0.1", port))
        s.sendall(req)
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
        s.close()
        lat.append((time.perf_counter() - t0) * 1e6)  # microseconds
    lat.sort()
    return lat


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 300
    for name, port in [("direct upstream :8080", 8080), ("through proxy   :9000", 9000)]:
        l = bench(port, n)
        p = lambda q: l[int(q * len(l)) - 1]
        print(
            f"{name}  median={statistics.median(l):8.1f}us  "
            f"p99={p(0.99):8.1f}us  min={l[0]:8.1f}us"
        )


if __name__ == "__main__":
    main()
