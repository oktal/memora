use std::{fmt, future, io};

use futures::{future::BoxFuture, Future, Sink, SinkExt, Stream, StreamExt};
use rand::Rng;
use thiserror::Error;
use tokio::net::ToSocketAddrs;
use tokio_util::codec::{Decoder, Framed};
use tracing::{debug, info};

use crate::resp::{self, RespError, RespResult};

use super::{framer::RespFramer, MemoraError, MemoraResult};

#[derive(Debug, Error)]
pub enum HandshakeError {
    #[error(transparent)]
    Io(#[from] io::Error),

    #[error(transparent)]
    Resp(#[from] resp::RespError),

    #[error("connection has been closed prematurely")]
    Closed,

    #[error("got an invalid response from master")]
    InvalidResponse(resp::Value),
}

#[derive(Debug, Error)]
pub enum ReplicaError {
    #[error("error handshaking with master node: {0}")]
    Handshare(#[from] HandshakeError),
}

pub trait Role {
    type StartFuture: Future<Output = MemoraResult<()>>;

    fn info(&self) -> Vec<String>;
    fn start(&mut self) -> Self::StartFuture;
}

#[derive(Debug)]
struct ReplicationId([u8; 40]);

impl fmt::Display for ReplicationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: this is safe because we generated a ReplicationId from valid alphanumeric characters
        let str = unsafe { std::str::from_utf8_unchecked(&self.0) };
        f.write_str(str)
    }
}

impl ReplicationId {
    fn random() -> Self {
        let chars = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(40)
            .collect::<Vec<_>>();

        Self(chars.try_into().expect(
            "taking 40 values from an iterator should be convertible to an array of 40 elements",
        ))
    }
}

pub struct Master {
    id: ReplicationId,
    offset: usize,
}

pub struct Replica {
    master_port: u16,
    addr: (String, u16),
}

impl Master {
    pub fn new() -> Self {
        Self {
            id: ReplicationId::random(),
            offset: 0,
        }
    }
}

impl Replica {
    pub fn of(master_port: u16, host: impl Into<String>, port: impl Into<u16>) -> Self {
        Self {
            master_port,
            addr: (host.into(), port.into()),
        }
    }
}

impl Role for Master {
    type StartFuture = future::Ready<MemoraResult<()>>;

    fn info(&self) -> Vec<String> {
        let fields = [
            ("role", "master".to_owned()),
            ("master_replid", self.id.to_string()),
            ("master_repl_offset", self.offset.to_string()),
        ];

        fields
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect()
    }

    fn start(&mut self) -> Self::StartFuture {
        future::ready(Ok(()))
    }
}

async fn replconf<S, Args>(mut conn: S, args: Args) -> Result<(), HandshakeError>
where
    S: Sink<resp::Value, Error = RespError> + Stream<Item = RespResult<resp::Value>> + Unpin,
    Args: IntoIterator<Item = resp::Value>,
{
    // Create the `REPLCONF` command
    let replconf =
        resp::Value::from_iter(std::iter::once(resp::Value::bulk("REPLCONF")).chain(args));

    // Send it
    conn.send(replconf).await?;

    // Make sure we received an OK
    let resp = conn.next().await.ok_or(HandshakeError::Closed)??;
    let Some(ok) = resp.as_str() else {
        return Err(HandshakeError::InvalidResponse(resp));
    };

    if !ok.eq_ignore_ascii_case("ok") {
        return Err(HandshakeError::InvalidResponse(resp));
    }

    Ok(())
}

async fn handshake(
    master_addr: impl ToSocketAddrs,
    port: u16,
) -> Result<Framed<tokio::net::TcpStream, RespFramer>, HandshakeError> {
    // Connect to the master
    let conn = tokio::net::TcpStream::connect(master_addr).await?;

    // Frame the connection
    let mut conn = RespFramer.framed(conn);

    // Step 1. Send a PING to the master and wait for an answer
    debug!("sending `PING` to master node...");
    let ping = resp::Value::from_iter([resp::Value::bulk("PING")]);
    conn.send(ping).await?;

    // Attempt to read response from handshake
    let _resp = conn.next().await.ok_or(HandshakeError::Closed)??;
    // TODO(oktal): check that the response is a valid response from a PING

    // Step 2. Send the first REPLCONF message to configure the port the replica is listening to
    replconf(
        &mut conn,
        [
            resp::Value::bulk("listening-port"),
            resp::Value::bulk(port.to_string()),
        ],
    )
    .await?;

    // Step 3. Send the second REPLCONF to configure the capabilities of the replica
    replconf(
        &mut conn,
        [resp::Value::bulk("capa"), resp::Value::bulk("psync2")],
    )
    .await?;

    // Handshake is done
    Ok(conn)
}

impl Role for Replica {
    type StartFuture = BoxFuture<'static, MemoraResult<()>>;

    fn info(&self) -> Vec<String> {
        let fields = [("role", "slave")];
        fields
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect()
    }

    fn start(&mut self) -> Self::StartFuture {
        info!("connecting to {}:{} ...", self.addr.0, self.addr.1);

        let addr = self.addr.clone();
        let master_port = self.master_port;

        Box::pin(async move {
            // Initiate handshake
            handshake(addr, master_port)
                .await
                .map_err(|e| MemoraError::Standard(Box::new(e)))?;
            Ok(())
        })
    }
}
