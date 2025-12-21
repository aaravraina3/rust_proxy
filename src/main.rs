use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;
use rust_proxy::proxy;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Address to listen on
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    listen: String,

    /// Target address to forward to
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    target: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    let args = Args::parse();

    proxy::run_proxy(&args.listen, &args.target).await?;

    Ok(())
}
