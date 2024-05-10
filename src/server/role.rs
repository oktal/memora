use std::{fmt, future};

use futures::{future::BoxFuture, Future};
use rand::Rng;
use tracing::info;

use super::MemoraResult;

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
    pub fn of(host: impl Into<String>, port: impl Into<u16>) -> Self {
        Self {
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

        Box::pin(async move { Ok(()) })
    }
}
