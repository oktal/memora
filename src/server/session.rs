use futures::SinkExt;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Framed};
use tracing::{error, info};

use crate::resp::{StringValue, Value};

use super::{cmd::Command, framer::RespFramer, MemoraError, MemoraResult, Request, Response};

pub(super) struct Session {
    conn: Framed<tokio::net::TcpStream, RespFramer>,
    reqs_tx: mpsc::Sender<Request>,
}

impl Session {
    pub(super) fn new(conn: tokio::net::TcpStream, reqs_tx: mpsc::Sender<Request>) -> Self {
        Self {
            conn: RespFramer.framed(conn),
            reqs_tx,
        }
    }

    pub(super) async fn run(mut self) -> MemoraResult<()> {
        loop {
            let Some(Ok(value)) = self.conn.next().await else {
                break;
            };

            let command = Command::try_from(value);

            let res = match command {
                Ok(cmd) => self.handle_command(cmd).await,
                Err(e) => Err(MemoraError::Command(e)),
            };

            if let Err(e) = res {
                error!("failed to handle message: {e}");
            }
        }

        Ok(())
    }

    async fn handle_command(&mut self, cmd: Command) -> MemoraResult<()> {
        info!("handling {cmd:?}");

        let resp = match cmd {
            Command::Ping(msg) => {
                if let Some(msg) = msg {
                    Value::from_iter([Value::bulk("PONG"), Value::bulk(msg)]).into()
                } else {
                    Value::Str(StringValue::Simple("PONG".to_owned())).into()
                }
            }

            Command::Echo(msg) => Value::bulk(msg).into(),

            cmd => {
                let (req, rx) = Request::new(cmd);
                let _ = self.reqs_tx.send(req).await;

                // TODO(oktal): properly handle channel closing
                let resp = rx.await.unwrap();
                resp
            }
        };

        self.conn.send(resp).await?;
        Ok(())
    }
}
