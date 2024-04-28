use std::io;

use thiserror::Error;

pub mod cmd;
pub use cmd::Command;
pub mod redis;

pub use redis::Redis;

#[derive(Debug, Error)]
pub enum RedisError {
    #[error("{0}")]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, RedisError>;
