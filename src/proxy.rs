use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info, warn};
use httparse::Request;

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

    let (mut client_recv, mut client_send) = client_stream.split();
    let (mut server_recv, mut server_send) = server_stream.split();

    // Task for Client -> Server (with inspection and modification)
    let client_to_server = async {
        let mut buffer = [0u8; 8192];
        loop {
            let n = client_recv.read(&mut buffer).await?;
            if n == 0 { break; }

            let mut data = buffer[..n].to_vec();
            
            // Inspect and Modify
            if let Some(modified_data) = inspect_and_modify(&data) {
                data = modified_data;
            }

            server_send.write_all(&data).await?;
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

    tokio::try_join!(client_to_server, server_to_client)?;

    info!("Connection closed");
    Ok(())
}

fn inspect_and_modify(data: &[u8]) -> Option<Vec<u8>> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);

    match req.parse(data) {
        Ok(httparse::Status::Complete(_)) => {
            let method = req.method.unwrap_or("UNKNOWN");
            let path = req.path.unwrap_or("UNKNOWN");
            info!("HTTP Request: {} {}", method, path);

            // Simple modification: Inject a header if it looks like an HTTP request
            // Note: This is a naive implementation that assumes the entire header is in the first packet
            let data_str = String::from_utf8_lossy(data);
            if data_str.contains("\r\n\r\n") {
                let mut modified = data_str.replace("\r\n\r\n", "\r\nX-Proxy-Handled: true\r\n\r\n");
                return Some(modified.into_bytes());
            }
        }
        Ok(httparse::Status::Partial) => {
            // Partial header, could wait for more but for now just pass through
        }
        Err(_) => {
            // Not HTTP or malformed, just pass through
        }
    }

    None
}
