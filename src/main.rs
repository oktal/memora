// Uncomment this block to pass the first stage
// use std::net::TcpListener;

use redis::Redis;
use tracing::info;

mod redis;

const DEFAULT_HOSTNAME: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 6379;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    info!("binding to {DEFAULT_HOSTNAME}:{DEFAULT_PORT}");

    let redis = Redis::new((DEFAULT_HOSTNAME, DEFAULT_PORT)).await?;
    redis.start().await?;

    Ok(())
}
