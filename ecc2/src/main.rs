#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::field_reassign_with_default)]
#![allow(clippy::type_complexity)]
#![allow(clippy::cloned_ref_to_slice_refs)]
mod comms;
mod config;
mod notifications;
mod observability;
mod session;
mod tui;
mod worktree;
pub mod cli;
pub mod commands;

#[cfg(test)]
pub(crate) mod test_support;

use clap::Parser;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = cli::Cli::parse();
    let cfg = config::Config::load()?;
    let db = session::store::StateStore::open(&cfg.db_path)?;

    commands::execute(cli, cfg, db).await
}
