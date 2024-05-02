#![allow(dead_code)]

use std::io::Write;

use logos::Logos;

use super::{RedisError, RespError, Result};

/// A valid token from Redis Serialization Protocol
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

impl Token {
    fn expect_int(&self) -> Result<i64> {
        match self {
            Self::Int(v) => Ok(*v),
            _ => Err(RedisError::Resp(RespError::InvalidToken)),
        }
    }
}

/// Represents a string value
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum StringValue {
    /// Simple strings transmit short, non-binary strings with minimal overhead.
    /// For example, many Redis commands reply with just "OK" on success.
    Simple(String),

    /// A bulk string represents a single binary string. The string can be of any size, but by default, Redis limits it to 512 MB
    /// A value of [`None`] represents a null bulk string
    Bulk(Option<String>),

    /// A null string
    Null,
}

impl StringValue {
    fn encode(&self, buf: &mut impl Write) -> Result<()> {
        Ok(match self {
            Self::Simple(str) => {
                write!(buf, "+{str}")
            }

            Self::Bulk(Some(str)) => {
                let len = str.len();
                write!(buf, "${len}\r\n{str}")
            }
            Self::Null | Self::Bulk(None) => write!(buf, "$-1"),
        }?)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Simple(str) => Some(str.as_str()),
            Self::Bulk(str) => str.as_ref().map(|s| s.as_str()),
            _ => None,
        }
    }
}

/// A value corresponding to the Redis Serialization Protocol.
/// RESP can serialize different data types including integers, strings, and arrays.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Value {
    /// Clients send commands to the Redis server as RESP arrays.
    Array(Vec<Value>),

    /// A string value
    Str(StringValue),

    /// CRLF-terminated string that represents a signed, base-10, 64-bit integer.
    Int(i64),
}

impl Value {
    /// Create a new [`Value`] representing a non-null bulk string
    pub fn bulk(s: impl Into<String>) -> Self {
        Self::Str(StringValue::Bulk(Some(s.into())))
    }

    /// Create a new [`Value`] representing a null bulk string
    pub fn null_bulk() -> Self {
        Self::Str(StringValue::Bulk(None))
    }

    /// Creata a new [`Value`] representing an array of values
    pub fn from_iter<I, V>(it: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Self>,
    {
        Self::Array(it.into_iter().map(Into::into).collect())
    }

    pub fn encode(&self, buf: &mut impl Write) -> Result<()> {
        match self {
            Self::Array(values) => {
                let len = values.len();
                write!(buf, "*{len}\r\n")?;

                for value in values {
                    value.encode(buf)?;
                }

                Ok(())
            }

            Self::Str(s) => {
                s.encode(buf)?;
                write!(buf, "\r\n")
            }

            Self::Int(i) => Ok(write!(buf, "{i}\r\n")?),
        }?;

        Ok(())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(str) => str.as_str(),
            _ => None,
        }
    }
}

pub(super) struct Parser<L> {
    lexer: L,
}

impl<L> Parser<L>
where
    L: Iterator<Item = std::result::Result<Token, ()>>,
{
    pub(super) fn new(lexer: L) -> Self {
        Self { lexer }
    }

    fn parse_bulk(&mut self) -> Result<Option<String>> {
        let length = self.next()?.expect_int()?;

        if length == -1 {
            return Ok(None);
        }

        let length: usize = length
            .try_into()
            .map_err(|_| RespError::InvalidLength(length))?;

        let token = self.next()?;

        let str = match token {
            Token::Str(val) => val,
            Token::Int(val) => val.to_string(),
            _ => return Err(RedisError::Resp(RespError::InvalidToken)),
        };

        if str.len() != length {
            return Err(RedisError::Resp(RespError::LengthMismatch {
                expected: length,
                got: str.len(),
            }));
        }

        Ok(Some(str.to_owned()))
    }

    fn parse_array(&mut self) -> Result<Vec<Value>> {
        let length = self.next()?.expect_int()?;

        let length: usize = length
            .try_into()
            .map_err(|_| RespError::InvalidLength(length))?;

        Ok((0..length)
            .map(|_| self.parse().ok_or(RespError::UnexpectedEof)?)
            .collect::<Result<Vec<_>>>()?)
    }

    fn parse(&mut self) -> Option<Result<Value>> {
        let token = self.try_next()?;
        match token {
            Ok(tok) => match tok {
                Token::Star => {
                    let values = self.parse_array();
                    match values {
                        Ok(values) => Some(Ok(Value::Array(values))),
                        Err(e) => Some(Err(e)),
                    }
                }
                Token::Dollar => {
                    let bulk = self.parse_bulk();
                    match bulk {
                        Ok(str) => Some(Ok(Value::Str(StringValue::Bulk(str)))),
                        Err(e) => Some(Err(e)),
                    }
                }
                _ => todo!(),
            },
            Err(e) => Some(Err(e)),
        }
    }

    fn next(&mut self) -> Result<Token> {
        let tok = self.lexer.next().ok_or(RespError::UnexpectedEof)?;
        Ok(tok.map_err(|_| RespError::InvalidToken)?)
    }

    fn try_next(&mut self) -> Option<Result<Token>> {
        let tok = self.lexer.next()?;
        Some(tok.map_err(|_| RedisError::Resp(RespError::InvalidToken)))
    }
}

impl<L> Iterator for Parser<L>
where
    L: Iterator<Item = std::result::Result<Token, ()>>,
{
    type Item = Result<Value>;

    fn next(&mut self) -> Option<Self::Item> {
        self.parse()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use super::*;
    use logos::Logos;

    #[test]
    fn should_lex() {
        let expected = [
            Token::Star,
            Token::Int(2),
            Token::Dollar,
            Token::Int(4),
            Token::Str("echo".to_string()),
            Token::Dollar,
            Token::Int(3),
            Token::Str("hey".to_string()),
        ];

        let lex = Token::lexer("*2\r\n$4\r\necho\r\n$3\r\nhey\r\n");

        for (expected, tok) in expected.into_iter().zip(lex) {
            let tok = tok.expect(&format!("expected token {:?}", expected));
            assert_eq!(tok, expected);
        }
    }

    #[test]
    fn should_parse() {
        let lex = Token::lexer("*2\r\n$4\r\necho\r\n$3\r\nhey\r\n");
        let mut parser = Parser::new(lex);

        let value = parser.parse().expect("parse value").expect("parse value");
        assert_eq!(
            value,
            Value::from_iter([Value::bulk("echo"), Value::bulk("hey")])
        );
    }

    #[test]
    fn should_encode_value() {
        let value = Value::from_iter([Value::bulk("echo"), Value::bulk("hey")]);

        let mut buf = Cursor::new(Vec::new());
        let mut str = String::new();
        value.encode(&mut buf).expect("encode");

        buf.seek(SeekFrom::Start(0)).expect("seek to start");
        buf.read_to_string(&mut str).expect("read encoded value");

        assert_eq!(str, "*2\r\n$4\r\necho\r\n$3\r\nhey\r\n");
    }
}
