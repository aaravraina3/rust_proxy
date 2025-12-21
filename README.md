# TCP Proxy with Packet Inspection

A memory-safe network proxy in Rust that intercepts, inspects, and modifies TCP traffic in real-time.

## Why Rust for this?

This project exists because Python/Java can't do it well:

- **No garbage collection pauses** - GC languages randomly freeze to clean up memory. For a proxy handling thousands of connections, that's unacceptable latency spikes.
- **Zero-copy where possible** - We inspect packet data without allocating new memory for each packet. In Python, even looking at data creates copies.
- **Fearless concurrency** - Rust's ownership system guarantees no data races at compile time. In C++, you'd be debugging segfaults for days.
- **Async without overhead** - Tokio gives us epoll/kqueue efficiency without the callback hell of Node.js or the GIL of Python.

## How It Works

### The Architecture

```
[Client]                    [This Proxy]                    [Target Server]
    │                            │                                │
    ├─── TCP connect ───────────►│                                │
    │                            ├─── TCP connect ───────────────►│
    │                            │                                │
    ├─── HTTP request ──────────►│                                │
    │                            │── inspect ──┐                  │
    │                            │             │ parse headers    │
    │                            │◄─ modify ───┘ inject header    │
    │                            ├─── modified request ──────────►│
    │                            │                                │
    │                            │◄────────── response ───────────┤
    │◄─────── response ──────────┤                                │
```

### The Core Loop

```rust
// Two async tasks run simultaneously:
// 1. Client → Server (with inspection)
// 2. Server → Client (passthrough)

tokio::select! {
    result = client_to_server(&mut client_read, &mut server_write) => { ... }
    result = server_to_client(&mut server_read, &mut client_write) => { ... }
}
```

`tokio::select!` races both directions concurrently. Whichever has data ready gets processed. No threads blocked waiting.

### Packet Inspection

```rust
let mut headers = [httparse::EMPTY_HEADER; 32];
let mut req = httparse::Request::new(&mut headers);

match req.parse(&buffer[..n]) {
    Ok(httparse::Status::Complete(_)) => {
        // Full HTTP request parsed
        // req.method = Some("GET")
        // req.path = Some("/api/users")
    }
    Ok(httparse::Status::Partial) => {
        // Need more data, keep reading
    }
    Err(_) => {
        // Not HTTP, forward raw
    }
}
```

**Why `httparse`?** It's zero-allocation. The `headers` array is stack-allocated, and parsing happens in-place on the buffer. No heap allocations per request.

### Header Injection

```rust
fn inject_header(original: &[u8], header: &str) -> Vec<u8> {
    // Find end of first line (GET /path HTTP/1.1\r\n)
    // Insert our header right after
    // Return modified buffer
}
```

We inject `X-Proxy-Handled: true` into every HTTP request. The target server sees this header and knows the request came through our proxy.

### Memory Safety in Action

This code is impossible to write safely in C:

```rust
async fn handle_connection(mut client: TcpStream, target_addr: &str) {
    let mut server = TcpStream::connect(target_addr).await?;
    
    let (mut client_read, mut client_write) = client.split();
    let (mut server_read, mut server_write) = server.split();
    
    // Rust GUARANTEES at compile time:
    // - client_read and client_write can't be used from multiple tasks without sync
    // - When this function returns, both connections are properly closed
    // - No use-after-free, no double-free, no memory leaks
}
```

## What I Learned

### Async/Await in Rust

Rust's async is zero-cost abstraction. The compiler transforms async functions into state machines - no heap allocation for the future itself.

```rust
// This async block compiles to a state machine enum, not a heap-allocated closure
async {
    let data = socket.read(&mut buf).await;
    socket.write(&data).await;
}
```

### Ownership with Split Streams

```rust
let (read_half, write_half) = stream.split();
```

This is Rust's ownership system shining. You can't accidentally read and write from different threads without explicit synchronization because the compiler tracks who owns what.

### Error Handling

```rust
// No exceptions, no null pointers
// Every error is explicit in the type system
match connection.read(&mut buffer).await {
    Ok(0) => break,           // Connection closed
    Ok(n) => process(n),      // Got n bytes
    Err(e) => return Err(e),  // Propagate error
}
```

## Performance Characteristics

| Metric | Why It Matters |
|--------|----------------|
| Zero-copy parsing | Don't allocate memory per packet |
| Async I/O | Single thread handles thousands of connections |
| No GC | Predictable latency, no random pauses |
| Stack allocation | Headers parsed without heap allocation |

## Limitations / Future Work

- **No TLS interception** - Would need to MITM the TLS handshake
- **HTTP/1.1 only** - HTTP/2 is binary framed, different parsing
- **No connection pooling** - New connection to target per client connection
- **No rate limiting** - Could add token bucket algorithm

## Usage

```bash
# Terminal 1: Start a simple HTTP server on 8080
python3 -m http.server 8080

# Terminal 2: Run the proxy (listens on 9000, forwards to 8080)
cargo run -- --listen 127.0.0.1:9000 --target 127.0.0.1:8080

# Terminal 3: Make requests through the proxy
curl -v http://localhost:9000/

# You'll see the proxy log the request, and the server will receive
# the injected X-Proxy-Handled header
```

## Project Structure

```
src/
├── main.rs          # Entry point, CLI args, connection accept loop
├── proxy.rs         # Core proxy logic, bidirectional forwarding
├── inspect.rs       # HTTP parsing and header injection
└── lib.rs           # Module declarations
```

## Dependencies

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }   # Async runtime
httparse = "1"                                    # Zero-copy HTTP parsing
clap = { version = "4", features = ["derive"] }  # CLI argument parsing
```