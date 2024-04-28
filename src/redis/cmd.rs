use std::str::FromStr;

use logos::Logos;

use super::{
    resp::{Parser, Token, Value},
    RedisError, Result,
};

#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Command {
    Ping(Option<String>),
    Echo(String),
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

                let cmd = cmd.as_str();

                if cmd.eq_ignore_ascii_case("ping") {
                    let msg = match values.next() {
                        Some(value) => {
                            let Value::Str(msg) = value else {
                                return Err(RedisError::InvalidCommand);
                            };

                            Some(msg.as_str().to_owned())
                        }
                        _ => None,
                    };

                    Ok(Self::Ping(msg))
                } else if cmd.eq_ignore_ascii_case("echo") {
                    let msg = values.next().ok_or(RedisError::InvalidCommand)?;
                    let Value::Str(msg) = msg else {
                        return Err(RedisError::InvalidCommand);
                    };

                    Ok(Self::Echo(msg.as_str().to_owned()))
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
