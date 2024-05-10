use bytes::{Buf, BufMut, BytesMut};
use futures::SinkExt;
use logos::Logos;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_util::codec::{Decoder, Encoder, Framed};
use tracing::{error, info};

use crate::resp::{self, StringValue, Value};

use super::{cmd::Command, MemoraError, MemoraResult, Request, Response};

struct RespFramer;

impl Decoder for RespFramer {
    type Item = resp::Value;
    type Error = MemoraError;

    fn decode(&mut self, buf: &mut BytesMut) -> MemoraResult<Option<Self::Item>> {
        let src = std::str::from_utf8(&buf).map_err(|_| MemoraError::Utf8Error)?;
        let len = src.len();

        match resp::Value::parse(resp::Token::lexer(src)) {
            Ok(Some((value, remainder))) => {
                let parsed_len = len - remainder.len();
                buf.advance(parsed_len);
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(MemoraError::Resp(e)),
        }
    }
}

impl Encoder<resp::Value> for RespFramer {
    type Error = MemoraError;

    fn encode(&mut self, item: resp::Value, dst: &mut BytesMut) -> MemoraResult<()> {
        let mut writer = dst.writer();
        item.encode(&mut writer).map_err(MemoraError::Resp)
    }
}

impl Encoder<Response> for RespFramer {
    type Error = MemoraError;

    fn encode(
        &mut self,
        item: Response,
        dst: &mut BytesMut,
    ) -> std::prelude::v1::Result<(), Self::Error> {
        let mut writer = dst.writer();
        item.encode(&mut writer)
    }
}

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