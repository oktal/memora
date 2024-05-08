use clap::Parser;
use redis::{
    redis::{Master, Replica},
    Redis,
};
use tokio::net::ToSocketAddrs;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::opts::Opts;

mod opts;
mod redis;

const DEFAULT_HOSTNAME: &str = "127.0.0.1";

async fn run<R>(addr: impl ToSocketAddrs, role: R) -> anyhow::Result<()>
where
    R: redis::Role,
{
    let redis = Redis::new(addr, role).await?;
    redis.start().await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_env("REDIS_LOG"))
        .init();

    let opts = Opts::parse();
    let addr = (DEFAULT_HOSTNAME, opts.port);
    if let Some((host, port)) = opts.replica_of()? {
        run(addr, Replica::of(host, port)).await
    } else {
        run(addr, Master).await
    }?;

    Ok(())
}
