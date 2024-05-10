use std::io::{self};

use thiserror::Error;

use super::cmd::CommandError;
use crate::resp::RespError;

/// Type-alias for standard generic error type that is safe to shared across threads
pub type StdError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, Error)]
pub enum EncodeError {
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Error that can be raised by memora
#[derive(Debug, Error)]
pub enum MemoraError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Resp(#[from] RespError),

    #[error(transparent)]
    Encode(#[from] EncodeError),

    #[error(transparent)]
    Command(#[from] CommandError),

    #[error(transparent)]
    Standard(StdError),
}

pub type MemoraResult<T> = std::result::Result<T, MemoraError>;
