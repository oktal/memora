use std::{str::FromStr, time::Duration};

use logos::Logos;

use super::{
    resp::{Parser, Token, Value},
    CommandError, GetError, RedisError, Result, SetError,
};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Command {
    Ping(Option<String>),
    Echo(String),

    /// Set key to hold the string value.
    /// If key already holds a value, it is overwritten, regardless of its type.
    /// Any previous time to live associated with the key is discarded on successful SET operation.
    /// SET key value [NX | XX] [GET] [EX seconds | PX milliseconds | EXAT unix-time-seconds | PXAT unix-time-milliseconds | KEEPTTL]
    Set {
        key: String,
        value: String,
        expiry: Option<Duration>,
    },

    /// Get the value of key.
    /// If the key does not exist the special value nil is returned.
    /// An error is returned if the value stored at key is not a string, because GET only handles string values.
    /// GET key
    Get {
        key: String,
    },
}

impl FromStr for Command {
    type Err = RedisError;

    fn from_str(s: &str) -> Result<Self> {
        let lexer = Token::lexer(s);
        let mut parser = Parser::new(lexer);

        let value = parser.next().ok_or(RedisError::InvalidCommand)?;
        let value = value?;

        match value {
            Value::Array(values) => {
                let mut values = values.into_iter();
                let value = values.next().ok_or(RedisError::InvalidCommand)?;
                let Value::Str(cmd) = value else {
                    return Err(RedisError::InvalidCommand);
                };

                let cmd = cmd.as_str().ok_or(RedisError::InvalidCommand)?;

                if cmd.eq_ignore_ascii_case("ping") {
                    let msg = match values.next() {
                        Some(value) => {
                            let Value::Str(msg) = value else {
                                return Err(RedisError::Command(CommandError::InvalidArgument(
                                    value,
                                )));
                            };

                            Some(msg.as_str().unwrap_or("").to_owned())
                        }
                        _ => None,
                    };

                    Ok(Self::Ping(msg))
                } else if cmd.eq_ignore_ascii_case("echo") {
                    let msg = values.next().ok_or(RedisError::InvalidCommand)?;
                    let Value::Str(msg) = msg else {
                        return Err(RedisError::Command(CommandError::InvalidArgument(msg)));
                    };

                    Ok(Self::Echo(msg.as_str().unwrap_or("").to_owned()))
                } else if cmd.eq_ignore_ascii_case("set") {
                    let key = values
                        .next()
                        .ok_or(CommandError::Set(SetError::MissingKey))?;

                    let key = key
                        .as_str()
                        .ok_or(CommandError::InvalidArgument(key.clone()))?;

                    let value = values
                        .next()
                        .ok_or(CommandError::Set(SetError::MissingValue))?;

                    let value = value
                        .as_str()
                        .ok_or(CommandError::InvalidArgument(value.clone()))?;

                    // TODO(oktal): handle expiry and other set command options
                    Ok(Self::Set {
                        key: key.to_owned(),
                        value: value.to_owned(),
                        expiry: None,
                    })
                } else if cmd.eq_ignore_ascii_case("get") {
                    let key = values
                        .next()
                        .ok_or(CommandError::Get(GetError::MissingKey))?;

                    let key = key
                        .as_str()
                        .ok_or(CommandError::InvalidArgument(key.clone()))?;

                    Ok(Self::Get {
                        key: key.to_owned(),
                    })
                } else {
                    Err(RedisError::UnknownCommand(cmd.to_owned()))
                }
            }
            _ => Err(RedisError::InvalidCommand),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn should_parse_echo() {
        let cmd = "*2\r\n$4\r\necho\r\n$3\r\nhey\r\n"
            .parse::<Command>()
            .expect("parse echo");

        assert_eq!(cmd, Command::Echo("hey".to_owned()));
    }
}
