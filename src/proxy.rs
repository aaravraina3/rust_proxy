use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::inspect::inspect;

/// Cap on idle upstream connections we keep warm.
const MAX_IDLE: usize = 512;

/// A pool of keep-alive connections to the upstream. Reusing these is what makes
/// the proxy fast: without it we open (and tear down) a fresh upstream socket per
/// request, which exhausts ephemeral ports under load and stalls on TIME_WAIT.
#[derive(Clone)]
struct Pool {
    target: String,
    idle: Arc<Mutex<VecDeque<TcpStream>>>,
}

impl Pool {
    fn new(target: String) -> Self {
        Self {
            target,
            idle: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Reuse an idle upstream connection, or open a new one.
    async fn get(&self) -> Result<TcpStream> {
        if let Some(stream) = self.idle.lock().await.pop_front() {
            return Ok(stream);
        }
        let stream = TcpStream::connect(&self.target)
            .await
            .with_context(|| format!("connecting to upstream {}", self.target))?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }

    /// Return a still-healthy keep-alive connection to the pool.
    async fn put(&self, stream: TcpStream) {
        let mut idle = self.idle.lock().await;
        if idle.len() < MAX_IDLE {
            idle.push_back(stream);
        }
    }
}

pub async fn run_proxy(listen_addr: &str, target_addr: &str) -> Result<()> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", listen_addr))?;

    info!("Proxy listening on {}", listen_addr);
    info!("Forwarding to {}", target_addr);

    let pool = Pool::new(target_addr.to_string());

    loop {
        let (client, client_addr) = listener.accept().await?;
        client.set_nodelay(true).ok();
        let pool = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(client, pool).await {
                error!("connection {} ended: {}", client_addr, e);
            }
        });
    }
}

/// Serve one client connection, looping over keep-alive requests. Each request
/// borrows a pooled upstream connection and returns it when the response is
/// fully framed, so connections are reused instead of churned.
async fn handle_connection(mut client: TcpStream, pool: Pool) -> Result<()> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);

    loop {
        buf.clear();

        // Read the request head (everything up to the blank line).
        let head_len = match read_head(&mut client, &mut buf).await? {
            Some(n) => n,
            None => return Ok(()), // client closed cleanly between requests
        };

        // Parse once: zero-copy inspection plus framing decisions.
        let (client_keep_alive, body_len) = {
            let mut headers = [httparse::EMPTY_HEADER; 64];
            let mut req = httparse::Request::new(&mut headers);
            req.parse(&buf).context("parsing request")?;
            inspect(&buf[..head_len]); // httparse borrows from buf, no copy
            (
                keep_alive(req.version, req.headers),
                content_length(req.headers).unwrap_or(0),
            )
        };

        // Pull in any request body (e.g. POST) so the upstream gets the whole thing.
        let want = head_len + body_len;
        while buf.len() < want {
            let mut tmp = [0u8; 8192];
            let n = client.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
        }

        // Borrow an upstream connection, send the request, relay the response.
        let mut upstream = pool.get().await?;
        upstream.write_all(&buf[..buf.len().min(want)]).await?;

        let upstream_reusable = relay_response(&mut upstream, &mut client).await?;
        if upstream_reusable {
            pool.put(upstream).await;
        }

        if !client_keep_alive {
            return Ok(());
        }
    }
}

/// Read until the end of an HTTP request head (CRLF CRLF). Returns the head
/// length, or None if the peer closed before sending anything.
async fn read_head(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Option<usize>> {
    loop {
        // Try to parse what we have so far.
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);
        if let httparse::Status::Complete(n) = req.parse(buf)? {
            return Ok(Some(n));
        }

        let mut tmp = [0u8; 8192];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// Read the upstream response and stream it to the client. Returns whether the
/// upstream connection is safe to reuse (fully framed and keep-alive).
async fn relay_response(upstream: &mut TcpStream, client: &mut TcpStream) -> Result<bool> {
    let mut buf: Vec<u8> = Vec::with_capacity(8192);

    // Read response headers.
    let head_len = loop {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut resp = httparse::Response::new(&mut headers);
        match resp.parse(&buf)? {
            httparse::Status::Complete(n) => break n,
            httparse::Status::Partial => {}
        }
        let mut tmp = [0u8; 8192];
        let n = upstream.read(&mut tmp).await?;
        if n == 0 {
            // Closed before a full header: forward whatever we got, don't reuse.
            client.write_all(&buf).await?;
            return Ok(false);
        }
        buf.extend_from_slice(&tmp[..n]);
    };

    // Decide body framing and keep-alive from the parsed headers.
    let (len, chunked, ka) = {
        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut resp = httparse::Response::new(&mut headers);
        resp.parse(&buf)?;
        (
            content_length(resp.headers),
            is_chunked(resp.headers),
            keep_alive(resp.version, resp.headers),
        )
    };

    if let Some(len) = len {
        // Content-Length: forward exactly head + len bytes.
        let total = head_len + len;
        client.write_all(&buf).await?;
        let mut remaining = total.saturating_sub(buf.len());
        let mut tmp = [0u8; 8192];
        while remaining > 0 {
            let n = upstream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(false);
            }
            client.write_all(&tmp[..n]).await?;
            remaining = remaining.saturating_sub(n);
        }
        Ok(ka)
    } else if chunked {
        // Transfer-Encoding: chunked. Pump until the terminating 0-length chunk.
        client.write_all(&buf).await?;
        let mut window = buf;
        while !ends_chunked(&window) {
            let mut tmp = [0u8; 8192];
            let n = upstream.read(&mut tmp).await?;
            if n == 0 {
                return Ok(false);
            }
            client.write_all(&tmp[..n]).await?;
            window.extend_from_slice(&tmp[..n]);
            // Only the tail matters for the terminator check.
            if window.len() > 8192 {
                window.drain(..window.len() - 8192);
            }
        }
        Ok(ka)
    } else {
        // No length and not chunked: body runs until EOF, so we can't reuse.
        client.write_all(&buf).await?;
        let mut tmp = [0u8; 8192];
        loop {
            let n = upstream.read(&mut tmp).await?;
            if n == 0 {
                break;
            }
            client.write_all(&tmp[..n]).await?;
        }
        Ok(false)
    }
}

fn header_value<'a>(headers: &'a [httparse::Header], name: &str) -> Option<&'a [u8]> {
    headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value)
}

fn content_length(headers: &[httparse::Header]) -> Option<usize> {
    let v = header_value(headers, "content-length")?;
    std::str::from_utf8(v).ok()?.trim().parse().ok()
}

fn is_chunked(headers: &[httparse::Header]) -> bool {
    header_value(headers, "transfer-encoding")
        .map(|v| v.to_ascii_lowercase().windows(7).any(|w| w == b"chunked"))
        .unwrap_or(false)
}

/// HTTP/1.1 defaults to keep-alive; HTTP/1.0 defaults to close. An explicit
/// Connection header overrides the default either way.
fn keep_alive(version: Option<u8>, headers: &[httparse::Header]) -> bool {
    if let Some(v) = header_value(headers, "connection") {
        let v = v.to_ascii_lowercase();
        if v.windows(5).any(|w| w == b"close") {
            return false;
        }
        if v.windows(10).any(|w| w == b"keep-alive") {
            return true;
        }
    }
    version == Some(1)
}

fn ends_chunked(buf: &[u8]) -> bool {
    buf.ends_with(b"0\r\n\r\n")
}
