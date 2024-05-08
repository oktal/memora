use std::io::{self, Write};

use thiserror::Error;

pub mod cmd;
pub mod redis;
pub mod resp;
mod session;

pub use redis::{Redis, Role};
use tokio::sync::oneshot;

use self::{
    cmd::Command,
    resp::{StringValue, Value},
};

#[derive(Debug, Error)]
pub enum SetError {
    #[error("missing key for `SET` command")]
    MissingKey,

    #[error("missing value for `SET` command")]
    MissingValue,

    #[error("missing expiry timestamp for `SET` command")]
    MissingExpiry,
}

#[derive(Debug, Error)]
pub enum GetError {
    #[error("missing key for `GET` command")]
    MissingKey,
}

#[derive(Debug, Error)]
pub enum InfoError {
    #[error("unknown section {0} for `INFO` command")]
    UnknownSection(String),
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Set(#[from] SetError),

    #[error(transparent)]
    Get(#[from] GetError),

    #[error(transparent)]
    Info(#[from] InfoError),

    #[error("invalid argument for command: {0:?}")]
    InvalidArgument(Value),
}

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum RespError {
    #[error("invalid token encoutered")]
    InvalidToken,

    #[error("invalid length {0}")]
    InvalidLength(i64),
}

#[derive(Debug, Error)]
pub enum RedisError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Resp(#[from] RespError),

    #[error(transparent)]
    Encode(#[from] EncodeError),

    #[error("invalid command")]
    InvalidCommand,

    #[error("unknown command {0}")]
    UnknownCommand(String),

    #[error("invalid utf-8 sequence")]
    Utf8Error,

    #[error(transparent)]
    Command(#[from] CommandError),
}

pub type Result<T> = std::result::Result<T, RedisError>;

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

pub struct Response(Value);

impl Response {
    fn encode(&self, buf: &mut impl Write) -> Result<()> {
        self.0.encode(buf)
    }

    pub fn ok() -> Self {
        Value::Str(StringValue::Simple("OK".to_owned())).into()
    }
}

impl From<Value> for Response {
    fn from(value: Value) -> Self {
        Self(value)
    }
}
