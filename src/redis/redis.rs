use std::{
    collections::{hash_map::Entry, HashMap},
    net::SocketAddr,
};

use crate::redis::session::Session;

use super::{
    cmd::Command,
    resp::{StringValue, Value},
    Request, Response, Result,
};
use tokio::{net::ToSocketAddrs, sync::mpsc};
use tracing::{error, info};

pub struct Redis {
    listener: tokio::net::TcpListener,
    sessions: Vec<tokio::task::JoinHandle<Result<()>>>,

    kvs: HashMap<String, String>,
}

impl Redis {
    pub async fn new(addr: impl ToSocketAddrs) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(addr).await?;

        Ok(Self {
            listener,
            sessions: Vec::new(),
            kvs: HashMap::new(),
        })
    }

    pub async fn start(mut self) -> Result<()> {
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
            Command::Set { key, value, .. } => {
                match self.kvs.entry(key) {
                    Entry::Occupied(mut e) => *e.get_mut() = value,
                    Entry::Vacant(e) => {
                        e.insert(value);
                    }
                }
                Ok(Value::Str(StringValue::Simple("OK".to_owned())).into())
            }
            Command::Get { key } => Ok(if let Some(value) = self.kvs.get(&key) {
                Value::bulk(value.clone())
            } else {
                Value::null_bulk()
            }
            .into()),
            _ => todo!(),
        }
    }
}
