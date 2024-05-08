use clap::Parser;
use redis::Redis;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::opts::Opts;

mod opts;
mod redis;

const DEFAULT_HOSTNAME: &str = "127.0.0.1";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_env("REDIS_LOG"))
        .init();

    let opts = Opts::parse();

    let redis = Redis::new((DEFAULT_HOSTNAME, opts.port)).await?;
    redis.start().await?;

    Ok(())
}
