use anyhow::bail;
use clap::Parser;

pub const DEFAULT_PORT: u16 = 6379;

/// Command-line option parameters
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// Port to bind the server to
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,

    /// Set this instance to be replica of an other server
    #[arg(long, value_delimiter = ' ', num_args = 2)]
    pub replicaof: Option<Vec<String>>,
}

impl Opts {
    pub fn replica_of(&self) -> anyhow::Result<Option<(String, u16)>> {
        let Some(addr) = self.replicaof.as_deref() else {
            return Ok(None);
        };

        let mut addr = addr.iter();

        let Some(host) = addr.next().cloned() else {
            bail!("invalid format for replicaof address. Valid format is hostname port (example localhost 6379)");
        };

        let Some(port) = addr.next() else {
            bail!("invalid format for replicaof address. Valid format is hostname port (example localhost 6379)");
        };

        Ok(Some((host, port.parse()?)))
    }
}
