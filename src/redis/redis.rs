use super::Result;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::ToSocketAddrs,
};
use tracing::{debug, info};

async fn handle_connection(mut stream: tokio::net::TcpStream) -> Result<()> {
    let mut buf = [0u8; 512];

    loop {
        let bytes = stream.read(&mut buf).await?;

        if bytes == 0 {
            break;
        }

        debug!("received {:?}", &buf[..bytes]);
        stream.write(b"+PONG\r\n").await?;
    }

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
