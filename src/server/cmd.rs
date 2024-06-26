use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use thiserror::Error;

use crate::resp::{self, Value};

#[derive(Debug, Error)]
pub enum SetError {
    #[error("missing key for `SET` command")]
    MissingKey,

    #[error("missing value for `SET` command")]
    MissingValue,

    #[error("missing expiry timestamp for `SET` command")]
    MissingExpiry,
}

#[derive(Debug, Error)]
pub enum GetError {
    #[error("missing key for `GET` command")]
    MissingKey,
}

#[derive(Debug, Error)]
pub enum InfoError {
    #[error("unknown section {0} for `INFO` command")]
    UnknownSection(String),
}

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Set(#[from] SetError),

    #[error(transparent)]
    Get(#[from] GetError),

    #[error(transparent)]
    Info(#[from] InfoError),

    #[error("invalid argument for command: {0:?}")]
    InvalidArgument(resp::Value),

    #[error("invalid command")]
    InvalidCommand,

    #[error("unknown command {0}")]
    UnknownCommand(String),
}

pub type CommandResult<T> = std::result::Result<T, CommandError>;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Time {
    Seconds(u64),
    Millis(u64),
}

impl Into<Duration> for Time {
    fn into(self) -> Duration {
        match self {
            Self::Seconds(secs) => Duration::from_secs(secs),
            Self::Millis(millis) => Duration::from_millis(millis),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Expiry {
    Time(Time),
    Unix(Time),
}

impl Expiry {
    /// Turn this raw expiry time a UTC [`chrono::DateTime`]
    pub(crate) fn into_utc(self) -> Option<DateTime<Utc>> {
        match self {
            Self::Time(time) => {
                let now = Utc::now();
                let delta = TimeDelta::from_std(time.into()).ok()?;
                Some(now + delta)
            }

            Self::Unix(ts) => match ts {
                Time::Seconds(secs) => DateTime::from_timestamp(secs as i64, 0),
                Time::Millis(millis) => DateTime::from_timestamp_millis(millis as i64),
            },
        }
    }
}

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
        expiry: Option<Expiry>,
    },

    /// Get the value of key.
    /// If the key does not exist the special value nil is returned.
    /// An error is returned if the value stored at key is not a string, because GET only handles string values.
    /// GET key
    Get {
        key: String,
    },

    /// The INFO command returns information and statistics about the server in a format that is simple to parse by
    /// computers and easy to read by humans.
    Info {
        /// The optional parameter can be used to select a specific section of information
        section: Option<String>,
    },
}

impl TryFrom<Value> for Command {
    type Error = CommandError;

    fn try_from(value: Value) -> CommandResult<Self> {
        match value {
            Value::Array(values) => {
                let mut values = values.into_iter();
                let value = values.next().ok_or(CommandError::InvalidCommand)?;
                let Value::Str(cmd) = value else {
                    return Err(CommandError::InvalidCommand);
                };

                let cmd = cmd.as_str().ok_or(CommandError::InvalidCommand)?;

                if cmd.eq_ignore_ascii_case("ping") {
                    let msg = match values.next() {
                        Some(value) => {
                            let Value::Str(msg) = value else {
                                return Err(CommandError::InvalidArgument(value));
                            };

                            Some(msg.as_str().unwrap_or("").to_owned())
                        }
                        _ => None,
                    };

                    Ok(Self::Ping(msg))
                } else if cmd.eq_ignore_ascii_case("echo") {
                    let msg = values.next().ok_or(CommandError::InvalidCommand)?;
                    let Value::Str(msg) = msg else {
                        return Err(CommandError::InvalidArgument(msg));
                    };

                    Ok(Self::Echo(msg.as_str().unwrap_or("").to_owned()))
                } else if cmd.eq_ignore_ascii_case("set") {
                    let Some(key) = values.next() else {
                        return Err(CommandError::Set(SetError::MissingKey));
                    };

                    let Some(key) = key.as_str() else {
                        return Err(CommandError::InvalidArgument(key));
                    };

                    let Some(value) = values.next() else {
                        return Err(CommandError::Set(SetError::MissingValue));
                    };

                    let Some(value) = value.as_str() else {
                        return Err(CommandError::InvalidArgument(value));
                    };

                    let expiry = if let Some(arg) = values.next() {
                        let Some(expiry_key) = arg.as_str() else {
                            return Err(CommandError::InvalidArgument(arg));
                        };

                        let Some(expiry_value) = values.next() else {
                            return Err(CommandError::Set(SetError::MissingExpiry));
                        };

                        let Some(expiry) = expiry_value.as_str() else {
                            return Err(CommandError::InvalidArgument(expiry_value));
                        };

                        let expiry: u64 = expiry
                            .parse()
                            .map_err(|_| CommandError::InvalidArgument(expiry_value))?;

                        if expiry_key.eq_ignore_ascii_case("ex") {
                            Some(Expiry::Time(Time::Seconds(expiry)))
                        } else if expiry_key.eq_ignore_ascii_case("px") {
                            Some(Expiry::Time(Time::Millis(expiry)))
                        } else if expiry_key.eq_ignore_ascii_case("exat") {
                            Some(Expiry::Unix(Time::Seconds(expiry)))
                        } else if expiry_key.eq_ignore_ascii_case("pxat") {
                            Some(Expiry::Unix(Time::Millis(expiry)))
                        } else {
                            return Err(CommandError::InvalidArgument(arg));
                        }
                    } else {
                        None
                    };

                    Ok(Self::Set {
                        key: key.to_owned(),
                        value: value.to_owned(),
                        expiry,
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
                } else if cmd.eq_ignore_ascii_case("info") {
                    let section = match values.next() {
                        Some(section) => Some(
                            section
                                .as_str()
                                .map(|s| s.to_owned())
                                .ok_or(CommandError::InvalidArgument(section))?,
                        ),
                        None => None,
                    };

                    Ok(Self::Info { section })
                } else {
                    Err(CommandError::UnknownCommand(cmd.to_owned()))
                }
            }
            _ => Err(CommandError::InvalidCommand),
        }
    }
}
