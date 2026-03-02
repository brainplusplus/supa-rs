pub mod start;
pub mod stop;
pub mod status;
pub mod logs;

use std::path::PathBuf;

#[derive(clap::Parser)]
#[command(name = "suparust", about = "SupaRust — Supabase-compatible backend")]
pub struct Cli {
    /// Load environment from .env.<profile> (e.g. --profile test loads .env.test)
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Load environment from a specific file path (advanced; cannot combine with --profile)
    #[arg(long = "env-file", global = true)]
    pub env_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    /// Start the server (foreground by default)
    Start {
        /// Run as background daemon, logs → app.log
        #[arg(long)]
        daemon: bool,

        /// Internal: child process spawned by --daemon (hidden)
        #[arg(long, hide = true)]
        daemon_child: bool,
    },
    /// Stop the running server
    Stop,
    /// Stop then restart as daemon
    Restart,
    /// Show server status (PID and port)
    Status,
    /// Tail app.log (daemon mode logs)
    Logs {
        /// Number of lines to show before following
        #[arg(long, default_value = "20")]
        lines: usize,
    },
}
