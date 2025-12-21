# Rust Memory-Safe Network Proxy

A high-performance TCP proxy built with Rust and Tokio.

## Features
- **Zero-copy parsing**: Uses `httparse` for efficient header inspection.
- **Async I/O**: Built on `tokio` to handle concurrent connections.
- **Packet Inspection**: Logs HTTP request methods and paths.
- **Packet Modification**: Injects a custom header (`X-Proxy-Handled`) into HTTP requests.

## Setup
1. Ensure you have Rust installed (`brew install rust`).
2. Clone the repository.

## Usage
Run the proxy with default settings (listens on `9000`, forwards to `8080`):
```bash
cargo run
```

Specify custom addresses:
```bash
cargo run -- --listen 127.0.0.1:9000 --target 127.0.0.1:8080
```

## How it works
1. The proxy listens for incoming TCP connections.
2. For each connection, it establishes a secondary connection to the target server.
3. Data is forwarded bidirectionally using async tasks.
4. Client-to-server traffic is inspected for HTTP patterns; if found, it logs the request and injects a custom header.

