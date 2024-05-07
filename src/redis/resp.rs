#![allow(dead_code)]

use std::io::Write;

use logos::{Lexer, Logos};

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

impl TryInto<String> for Token {
    type Error = RedisError;

    fn try_into(self) -> Result<String> {
        Ok(match self {
            Self::Int(v) => v.to_string(),
            Self::Str(s) => s,
            _ => return Err(RedisError::Resp(RespError::InvalidToken)),
        })
    }
}

impl Token {
    fn as_int(&self) -> Result<i64> {
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

    /// Parse a [`Self`] from a stream of [`Token`]
    pub fn parse<'a>(lexer: Lexer<'a, Token>) -> Result<Option<(Self, &'a str)>> {
        Parser::new(lexer).parse()
    }
}

struct Parser<'a> {
    lexer: Lexer<'a, Token>,
}

impl<'a> Parser<'a> {
    fn new(lexer: Lexer<'a, Token>) -> Self {
        Self { lexer }
    }

    /// Parse a RESP bulk string
    /// On success, the outer `Option` indicates whether a string has been parsed
    /// or not. The inner `Option` indicates whether the bulk string is a null string
    fn parse_bulk(&mut self) -> Result<Option<Option<String>>> {
        // Read length
        let Some(length) = self.try_next()? else {
            return Ok(None);
        };

        let Ok(length) = length.as_int() else {
            return Err(RedisError::Resp(RespError::InvalidToken));
        };

        // Null bulk string
        if length == -1 {
            return Ok(Some(None));
        }

        // This is not a bulk string, attempt to convert the length to a `usize`
        // and error otherwise
        let Ok(length) = length.try_into() else {
            return Err(RedisError::Resp(RespError::InvalidLength(length)));
        };

        // Read the string
        let Some(token) = self.try_next()? else {
            return Ok(None);
        };

        let str: String = token.try_into()?;

        // The length of the string we read does not match the expected length,
        // which means that we read a partial string
        if str.len() != length {
            return Ok(None);
        }

        Ok(Some(Some(str.to_owned())))
    }

    /// Attempt to parse a RESP array
    /// On success, return `Some` if a complete array has been parsed or `None`
    /// if a partial array has been parsed
    fn parse_array(&mut self) -> Result<Option<Vec<Value>>> {
        // Read length
        let Some(length) = self.try_next()? else {
            return Ok(None);
        };

        let Ok(length) = length.as_int() else {
            return Err(RedisError::Resp(RespError::InvalidToken));
        };

        let Ok(length) = length.try_into() else {
            return Err(RedisError::Resp(RespError::InvalidLength(length)));
        };

        let values = (0usize..length).map(|_| self.parse_one());
        values.collect()
    }

    fn parse(&mut self) -> Result<Option<(Value, &'a str)>> {
        match self.parse_one() {
            Ok(Some(value)) => Ok(Some((value, self.lexer.remainder()))),
            Ok(None) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Attempt to parse a RESP value
    /// On success, return `Some` if a complete value has been parsed or `None` if a partial
    /// value was parsed
    fn parse_one(&mut self) -> Result<Option<Value>> {
        let Some(token) = self.try_next()? else {
            return Ok(None);
        };

        match token {
            Token::Star => {
                let Some(values) = self.parse_array()? else {
                    return Ok(None);
                };
                Ok(Some(Value::Array(values)))
            }
            Token::Dollar => {
                let Some(bulk) = self.parse_bulk()? else {
                    return Ok(None);
                };
                Ok(Some(Value::Str(StringValue::Bulk(bulk))))
            }
            _ => todo!(),
        }
    }

    /// Attempt to consume the next [`Token`]
    /// On success, return `Some` if a token is available or `None` otherwise
    fn try_next(&mut self) -> Result<Option<Token>> {
        let Some(token) = self.lexer.next() else {
            return Ok(None);
        };

        token
            .map(Some)
            .map_err(|_| RedisError::Resp(RespError::InvalidToken))
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

        let value = parser
            .parse_one()
            .expect("parse value")
            .expect("parse value");
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
