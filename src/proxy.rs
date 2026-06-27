use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};
use crate::inspect::inspect;

pub async fn run_proxy(listen_addr: &str, target_addr: &str) -> Result<()> {
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", listen_addr))?;

    info!("Proxy listening on {}", listen_addr);
    info!("Forwarding to {}", target_addr);

    loop {
        let (client_stream, client_addr) = listener.accept().await?;
        info!("Accepted connection from {}", client_addr);

        let target_addr = target_addr.to_string();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(client_stream, &target_addr).await {
                error!("Error handling connection from {}: {}", client_addr, e);
            }
        });
    }
}

async fn handle_connection(mut client_stream: TcpStream, target_addr: &str) -> Result<()> {
    let mut server_stream = TcpStream::connect(target_addr)
        .await
        .with_context(|| format!("Failed to connect to target {}", target_addr))?;

    // Disable Nagle's algorithm on both legs: without this, small forwarded
    // writes wait for delayed ACKs, adding tens of ms per request under load.
    client_stream.set_nodelay(true)?;
    server_stream.set_nodelay(true)?;

    let (mut client_recv, mut client_send) = client_stream.split();
    let (mut server_recv, mut server_send) = server_stream.split();

    // Task for Client -> Server (zero-copy inspection, then forward as-is)
    let client_to_server = async {
        let mut buffer = [0u8; 8192];
        loop {
            let n = client_recv.read(&mut buffer).await?;
            if n == 0 { break; }

            // Parse in place: httparse borrows from the buffer, no allocation.
            inspect(&buffer[..n]);

            // Forward the original bytes; nothing is copied.
            server_send.write_all(&buffer[..n]).await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    // Task for Server -> Client (direct forwarding)
    let server_to_client = async {
        let mut buffer = [0u8; 8192];
        loop {
            let n = server_recv.read(&mut buffer).await?;
            if n == 0 { break; }
            client_send.write_all(&buffer[..n]).await?;
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        res = client_to_server => res,
        res = server_to_client => res,
    }?;

    info!("Connection closed");
    Ok(())
}
