use std::io::{self, Cursor, Seek, SeekFrom};

use crate::redis::RedisError;

use super::{
    cmd::Command,
    resp::{StringValue, Value},
    Result,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::ToSocketAddrs,
};
use tracing::{debug, error, info};

struct Buffer<W> {
    inner: W,
    count: usize,
}

impl<W> Buffer<W>
where
    W: io::Write,
{
    fn new(inner: W) -> Self {
        Self { inner, count: 0 }
    }
}

impl<W> io::Write for Buffer<W>
where
    W: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let len = self.inner.write(buf)?;
        self.count += len;
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

async fn handle_command(stream: &mut tokio::net::TcpStream, cmd: Command) -> Result<()> {
    info!("handling {cmd:?}");

    let resp = match cmd {
        Command::Ping(msg) => {
            if let Some(msg) = msg {
                Value::Array(vec![
                    Value::Str(StringValue::Bulk("PONG".to_owned())),
                    Value::Str(StringValue::Bulk(msg)),
                ])
            } else {
                Value::Str(StringValue::Simple("PONG".to_owned()))
            }
        }

        Command::Echo(msg) => Value::Str(StringValue::Bulk(msg)),
    };

    debug!("sending response {resp:?}");

    let mut buf = Buffer::new(Cursor::new(Vec::new()));
    resp.encode(&mut buf)?;

    let count = buf.count;
    let buf = buf.inner.into_inner();

    let buf = &buf[..count];

    debug!("sending {:?}", buf);

    stream.write(buf).await?;

    Ok(())
}

async fn handle_connection(mut stream: tokio::net::TcpStream) -> Result<()> {
    let mut buf = [0u8; 512];

    loop {
        let bytes = stream.read(&mut buf).await?;

        if bytes == 0 {
            break;
        }

        debug!("received {:?}", &buf[..bytes]);

        let res = match std::str::from_utf8(&buf[..bytes]) {
            Ok(resp) => match resp.parse::<Command>() {
                Ok(cmd) => handle_command(&mut stream, cmd).await,
                Err(e) => Err(e),
            },
            Err(_) => Err(RedisError::Utf8Error),
        };

        if let Err(e) = res {
            error!("failed to handle message: {e}");
        }
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
