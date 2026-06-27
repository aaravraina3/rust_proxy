// Minimal HTTP upstream used only for benchmarking the proxy. Responds to every
// request with a tiny fixed body. Honors Connection semantics (keep-alive vs
// close) so it isn't the bottleneck and plays nicely with both ab modes.
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:8080".into());
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("benchmark upstream on {addr}");

    let resp_ka = body_response("keep-alive");
    let resp_close = body_response("close");

    loop {
        let (mut sock, _) = listener.accept().await?;
        sock.set_nodelay(true).ok();
        let (resp_ka, resp_close) = (resp_ka.clone(), resp_close.clone());
        tokio::spawn(async move {
            let mut buf = [0u8; 8192];
            let mut acc: Vec<u8> = Vec::with_capacity(8192);
            loop {
                let n = match sock.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => n,
                };
                acc.extend_from_slice(&buf[..n]);
                while let Some(pos) = find_head_end(&acc) {
                    let keep = wants_keep_alive(&acc[..pos]);
                    let resp = if keep { &resp_ka } else { &resp_close };
                    if sock.write_all(resp).await.is_err() {
                        return;
                    }
                    acc.drain(..pos);
                    if !keep {
                        return; // client asked to close
                    }
                }
            }
        });
    }
}

fn body_response(connection: &str) -> Vec<u8> {
    let body = b"hello\n";
    format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\nConnection: {}\r\n\r\n",
        body.len(),
        connection
    )
    .into_bytes()
    .into_iter()
    .chain(body.iter().copied())
    .collect()
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn wants_keep_alive(head: &[u8]) -> bool {
    let lower = head.to_ascii_lowercase();
    let has = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
    if has(b"connection: close") {
        return false;
    }
    if has(b"connection: keep-alive") {
        return true;
    }
    has(b"http/1.1") // default: keep-alive only for HTTP/1.1
}
