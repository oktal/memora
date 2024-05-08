use clap::Parser;

pub const DEFAULT_PORT: u16 = 6379;

/// Command-line option parameters
#[derive(Debug, Clone, Parser)]
#[command(version, about, long_about = None)]
pub struct Opts {
    /// Port to bind the server to
    #[arg(long, default_value_t = DEFAULT_PORT)]
    pub port: u16,
}
