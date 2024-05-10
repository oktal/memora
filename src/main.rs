use clap::Parser;
use server::Memora;
use tokio::net::ToSocketAddrs;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::opts::Opts;

mod opts;
mod resp;
mod server;

const DEFAULT_HOSTNAME: &str = "127.0.0.1";

async fn run<R>(addr: impl ToSocketAddrs, role: R) -> anyhow::Result<()>
where
    R: server::Role,
{
    let memora = Memora::new(addr, role).await?;
    memora.start().await?;

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
        run(addr, server::role::Replica::of(host, port)).await
    } else {
        run(addr, server::role::Master::new()).await
    }?;

    Ok(())
}
