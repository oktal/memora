use std::io::{self};

use thiserror::Error;

use crate::resp::RespError;

use super::cmd::CommandError;

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

    #[error("invalid utf-8 sequence")]
    Utf8Error,

    #[error(transparent)]
    Command(#[from] CommandError),
}

pub type MemoraResult<T> = std::result::Result<T, MemoraError>;
