use logos::Logos;

use super::{RespError, RespResult};

/// Represents a token from the RESP protocol
#[derive(Debug, Eq, PartialEq, Clone, Logos)]
#[logos(skip r"[\r\n]+")]
pub enum Token {
    #[token("*")]
    Star,

    #[token("$")]
    Dollar,

    #[token("+")]
    Plus,

    #[regex(r"-?(?:0|[1-9]\d*)", |lex| lex.slice().parse::<i64>().expect("failed to parse integer"))]
    Int(i64),

    #[regex(r"[a-zA-Z]+", |lex| lex.slice().to_owned())]
    Str(String),
}

impl TryInto<String> for Token {
    type Error = RespError;

    fn try_into(self) -> RespResult<String> {
        Ok(match self {
            Self::Int(v) => v.to_string(),
            Self::Str(s) => s,
            _ => return Err(RespError::InvalidToken),
        })
    }
}

impl Token {
    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }
}
