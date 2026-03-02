pub mod config;
pub mod api;
pub mod db;
pub mod parser;
pub mod sql;
pub mod cli;
pub mod tracing;

use clap::Parser;
use cli::{Cli, Command};
use std::path::Path;

/// Determine a stable identity string used in PID file naming.
///
/// Rules:
///   --profile test        → "test"
///   --env-file .envgw     → "env.envgw"
///   --env-file prod.env   → "env.prod_env"
///   (no flags)            → "local"
pub fn derive_pid_identity(profile: Option<&str>, env_file: Option<&Path>) -> String {
    match (profile, env_file) {
        (Some(p), _) => p.to_string(),
        (None, Some(path)) => {
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            // Strip leading dots (e.g. ".envgw" → "envgw", ".env.test" → "env.test")
            let stripped = filename.trim_start_matches('.');
            let normalized: String = stripped
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            format!("env.{}", normalized)
        }
        (None, None) => "local".to_string(),
    }
}

fn load_env(profile: Option<&str>, env_file: Option<&Path>) {
    match (env_file, profile) {
        (Some(_), Some(_)) => {
            eprintln!("error: cannot use both --profile and --env-file");
            std::process::exit(1);
        }
        (Some(path), None) => {
            dotenvy::from_path(path).unwrap_or_else(|_| {
                eprintln!("error: env file not found: {}", path.display());
                std::process::exit(1);
            });
        }
        (None, Some(p)) => {
            let filename = format!(".env.{}", p);
            dotenvy::from_filename(&filename).unwrap_or_else(|_| {
                eprintln!("error: profile '{}' not found: {}", p, filename);
                std::process::exit(1);
            });
        }
        (None, None) => {
            dotenvy::dotenv().ok();
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    load_env(cli.profile.as_deref(), cli.env_file.as_deref());

    match cli.command {
        Some(Command::Start { daemon: true, .. }) => cli::start::cmd_start_daemon().await,
        Some(Command::Start { daemon_child: true, .. }) => cli::start::cmd_start_daemon_child().await,
        Some(Command::Start { .. }) | None => cli::start::cmd_start_foreground().await,
        Some(Command::Stop) => cli::stop::cmd_stop(),
        Some(Command::Restart) => {
            cli::stop::cmd_stop();
            cli::start::cmd_start_daemon().await;
        }
        Some(Command::Status) => cli::status::cmd_status(),
        Some(Command::Logs { lines }) => cli::logs::cmd_logs(lines),
    }

    Ok(())
}
