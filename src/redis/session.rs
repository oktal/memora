use std::io::{self};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};
use tracing::{debug, error, info};

use crate::redis::{
    cmd::Command,
    resp::{StringValue, Value},
    RedisError,
};

use super::{Request, Result};

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

pub(super) struct Session {
    conn: tokio::net::TcpStream,
    buf: Vec<u8>,
    reqs_tx: mpsc::Sender<Request>,
}

impl Session {
    pub(super) fn new(conn: tokio::net::TcpStream, reqs_tx: mpsc::Sender<Request>) -> Self {
        Self {
            conn,
            buf: Vec::with_capacity(1024),
            reqs_tx,
        }
    }

    pub(super) async fn run(mut self) -> Result<()> {
        let mut buf = [0u8; 512];

        loop {
            let bytes = self.conn.read(&mut buf).await?;

            if bytes == 0 {
                break;
            }

            debug!("received {:?}", &buf[..bytes]);

            let res = match std::str::from_utf8(&buf[..bytes]) {
                Ok(resp) => match resp.parse::<Command>() {
                    Ok(cmd) => self.handle_command(cmd).await,
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

    async fn handle_command(&mut self, cmd: Command) -> Result<()> {
        info!("handling {cmd:?}");

        let resp = match cmd {
            Command::Ping(msg) => {
                if let Some(msg) = msg {
                    Value::Array(vec![
                        Value::Str(StringValue::Bulk("PONG".to_owned())),
                        Value::Str(StringValue::Bulk(msg)),
                    ])
                    .into()
                } else {
                    Value::Str(StringValue::Simple("PONG".to_owned())).into()
                }
            }

            Command::Echo(msg) => Value::Str(StringValue::Bulk(msg)).into(),

            cmd => {
                let (req, rx) = Request::new(cmd);
                let _ = self.reqs_tx.send(req).await;

                // TODO(oktal): properly handle channel closing
                let resp = rx.await.unwrap();
                resp
            }
        };

        self.buf.clear();
        let mut buf = Buffer::new(&mut self.buf);
        resp.encode(&mut buf)?;

        let count = buf.count;
        let buf = &buf.inner[..count];

        debug!("sending {:?}", buf);

        self.conn.write(buf).await?;
        Ok(())
    }
}
