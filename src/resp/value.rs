use logos::Lexer;
use std::io::Write;

use super::{lex::Token, parser::Parser, RespResult};

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
    fn encode(&self, buf: &mut impl Write) -> RespResult<()> {
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
    pub fn bulk(s: impl ToString) -> Self {
        Self::Str(StringValue::Bulk(Some(s.to_string())))
    }

    /// Create a new [`Value`] representing a simple string
    pub fn simple(s: impl Into<String>) -> Self {
        Self::Str(StringValue::Simple(s.into()))
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

    pub fn encode(&self, buf: &mut impl Write) -> RespResult<()> {
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
    pub fn parse<'a>(lexer: Lexer<'a, Token>) -> RespResult<Option<(Self, &'a [u8])>> {
        Parser::new(lexer).parse()
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Seek, SeekFrom};

    use super::*;

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
