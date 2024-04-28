use std::io;

use thiserror::Error;
use tokio::net::ToSocketAddrs;
use tracing::info;

#[derive(Debug, Error)]
pub enum RedisError {
    #[error("{0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, RedisError>;

async fn handle_connection(_stream: tokio::net::TcpStream) -> Result<()> {
    Ok(())
}

pub struct Redis {
    listener: tokio::net::TcpListener,
}

impl Redis {
    pub async fn new(addr: impl ToSocketAddrs) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        Ok(Self { listener })
    }

    pub async fn start(self) -> Result<()> {
        loop {
            let (socket, addr) = self.listener.accept().await?;
            info!("got new connection from {addr:?}");
            tokio::spawn(handle_connection(socket));
        }
    }
}
