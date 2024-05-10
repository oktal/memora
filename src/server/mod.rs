//! Module that contains the main server implementation

mod cmd;
pub mod error;
use std::io::Write;

pub use error::{MemoraError, MemoraResult};
pub mod role;
pub use role::Role;
pub mod server;
pub use server::Memora;

mod session;
use session::Session;
use tokio::sync::oneshot;

use crate::resp;

use self::cmd::Command;

struct Request {
    cmd: Command,

    tx: oneshot::Sender<Response>,
}

impl Request {
    fn new(cmd: Command) -> (Self, oneshot::Receiver<Response>) {
        let (tx, rx) = oneshot::channel();
        (Self { cmd, tx }, rx)
    }
}

pub struct Response(resp::Value);

impl Response {
    fn encode(&self, buf: &mut impl Write) -> MemoraResult<()> {
        self.0.encode(buf).map_err(MemoraError::Resp)
    }

    pub fn ok() -> Self {
        resp::Value::Str(resp::StringValue::Simple("OK".to_owned())).into()
    }
}

impl From<resp::Value> for Response {
    fn from(value: resp::Value) -> Self {
        Self(value)
    }
}
