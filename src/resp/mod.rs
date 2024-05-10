//! This module contains components related to Redis [`RESP`](https://redis.io/docs/latest/develop/reference/protocol-spec/)
//! protocol specification

use std::io;

use thiserror::Error;

mod lex;
pub(self) mod parser;
pub mod value;

pub use lex::Token;
pub use value::{StringValue, Value};

/// Error that can be raised when encoding or decoding a RESP message
#[derive(Debug, Error)]
pub enum RespError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error("invalid utf-8 sequence")]
    Utf8Error,

    #[error("invalid token encoutered")]
    InvalidToken,

    #[error("invalid length {0}")]
    InvalidLength(i64),
}

/// A type-alias for a RESP [`std::result::Result`]
pub type RespResult<T> = std::result::Result<T, RespError>;
