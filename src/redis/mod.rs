use std::io;

use thiserror::Error;

pub mod cmd;
pub mod redis;
pub mod resp;

pub use redis::Redis;

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

    #[error("length mismatch. expected {expected} got {got}")]
    LengthMismatch { expected: usize, got: usize },

    #[error("unexpected end of file")]
    UnexpectedEof,
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
}

pub type Result<T> = std::result::Result<T, RedisError>;
