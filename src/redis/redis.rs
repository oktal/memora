use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};

use crate::redis::session::Session;

use super::{
    cmd::Command,
    resp::{StringValue, Value},
    CommandError, InfoError, RedisError, Request, Response, Result,
};
use chrono::Utc;
use futures::{
    future::{self, BoxFuture},
    Future,
};
use tokio::{net::ToSocketAddrs, sync::mpsc};
use tracing::{debug, error, info};

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
    ) -> Result<()> {
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

pub trait Role {
    type StartFuture: Future<Output = Result<()>>;

    fn info(&self) -> Vec<String>;
    fn start(&mut self) -> Self::StartFuture;
}

pub struct Master;
pub struct Replica {
    addr: (String, u16),
}

impl Replica {
    pub fn of(host: impl Into<String>, port: impl Into<u16>) -> Self {
        Self {
            addr: (host.into(), port.into()),
        }
    }
}

impl Role for Master {
    type StartFuture = future::Ready<Result<()>>;

    fn info(&self) -> Vec<String> {
        let fields = [("role", "master")];
        fields
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect()
    }

    fn start(&mut self) -> Self::StartFuture {
        future::ready(Ok(()))
    }
}

impl Role for Replica {
    type StartFuture = BoxFuture<'static, Result<()>>;

    fn info(&self) -> Vec<String> {
        let fields = [("role", "slave")];
        fields
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect()
    }

    fn start(&mut self) -> Self::StartFuture {
        info!("connecting to {}:{} ...", self.addr.0, self.addr.1);

        Box::pin(async move { Ok(()) })
    }
}

pub struct Redis<R> {
    listener: tokio::net::TcpListener,
    sessions: Vec<tokio::task::JoinHandle<Result<()>>>,

    role: R,

    string: StringStore,
}

impl<R> Redis<R>
where
    R: Role,
{
    pub async fn new(addr: impl ToSocketAddrs, role: R) -> Result<Self> {
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

    pub async fn start(mut self) -> Result<()> {
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

    async fn handle_command(&mut self, cmd: Command) -> Result<Response> {
        match cmd {
            Command::Info { section } => {
                let section = section.as_deref().unwrap_or("default");
                if section.eq_ignore_ascii_case("replication") {
                    let fields = self.role.info().join("\r\n");
                    Ok(Value::bulk(fields).into())
                } else {
                    Err(RedisError::Command(CommandError::Info(
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
