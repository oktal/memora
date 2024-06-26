use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};

use crate::resp::{StringValue, Value};

use super::{
    cmd::{Command, CommandError, InfoError},
    MemoraError, MemoraResult, Request, Response, Role,
};
use chrono::Utc;
use tokio::{net::ToSocketAddrs, sync::mpsc};
use tracing::{debug, error, info};

use super::Session;

#[derive(Debug)]
struct StringEntry {
    value: String,
    expiry: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Default)]
struct StringStore(HashMap<String, StringEntry>);

impl StringStore {
    pub(crate) fn store(
        &mut self,
        key: String,
        value: String,
        expiry: Option<chrono::DateTime<Utc>>,
    ) -> MemoraResult<()> {
        debug!("storing key {key} with value {value} and expiry {expiry:?}");

        match self.0.entry(key) {
            Entry::Occupied(mut e) => {
                let entry = e.get_mut();
                entry.expiry = expiry;
                entry.value = value;
            }
            Entry::Vacant(e) => {
                e.insert(StringEntry { value, expiry });
            }
        }
        Ok(())
    }

    pub(crate) fn try_get(
        &self,
        key: impl AsRef<str>,
        time: impl FnOnce() -> chrono::DateTime<Utc>,
    ) -> Option<&str> {
        let entry = self.0.get(key.as_ref())?;

        let expired = entry.expiry.map(|exp| exp <= time()).unwrap_or(false);

        // TODO(oktal): properly reclaim expired entry from memory
        if expired {
            None
        } else {
            Some(entry.value.as_str())
        }
    }
}

pub struct Memora<R> {
    listener: tokio::net::TcpListener,
    sessions: Vec<tokio::task::JoinHandle<MemoraResult<()>>>,

    role: R,

    string: StringStore,
}

impl<R> Memora<R>
where
    R: Role,
{
    pub async fn new(addr: impl ToSocketAddrs, role: R) -> MemoraResult<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;

        let addr = listener.local_addr()?;
        info!("listening on {addr}");

        Ok(Self {
            listener,
            sessions: Vec::new(),
            string: StringStore::default(),
            role,
        })
    }

    pub async fn start(mut self) -> MemoraResult<()> {
        self.role.start().await?;

        let (reqs_tx, mut reqs_rx) = mpsc::channel(128);

        loop {
            tokio::select! {
                conn = self.listener.accept() => {
                    let (socket, addr) = conn?;
                    self.handle_connection(socket, addr, reqs_tx.clone());
                }

                Some(req) = reqs_rx.recv() => {
                    let Request { cmd, tx } = req;
                    match self.handle_command(cmd).await {
                        Ok(resp) => {
                            let _ = tx.send(resp);
                        }
                        Err(e) => error!("error handling command: {e}"),
                    }

                }
            }
        }
    }

    fn handle_connection(
        &mut self,
        socket: tokio::net::TcpStream,
        addr: SocketAddr,
        reqs_tx: mpsc::Sender<Request>,
    ) {
        info!("got new connection from {addr:?}");

        let session = Session::new(socket, reqs_tx);
        self.sessions.push(tokio::spawn(session.run()));
    }

    async fn handle_command(&mut self, cmd: Command) -> MemoraResult<Response> {
        match cmd {
            Command::Info { section } => {
                let section = section.as_deref().unwrap_or("default");
                if section.eq_ignore_ascii_case("replication") {
                    let fields = self.role.info().join("\r\n");
                    Ok(Value::bulk(fields).into())
                } else {
                    Err(MemoraError::Command(CommandError::Info(
                        InfoError::UnknownSection(section.to_owned()),
                    )))
                }
            }
            Command::Set { key, value, expiry } => {
                let expiry = match expiry {
                    // TODO(oktal): properly handle error
                    Some(expiry) => Some(expiry.into_utc().expect("invalid expiry time")),
                    None => None,
                };
                self.string.store(key, value, expiry)?;
                Ok(Value::Str(StringValue::Simple("OK".to_owned())).into())
            }
            Command::Get { key } => Ok(
                if let Some(value) = self.string.try_get(&key, || Utc::now()) {
                    Value::bulk(value)
                } else {
                    Value::null_bulk()
                }
                .into(),
            ),
            _ => todo!(),
        }
    }
}
