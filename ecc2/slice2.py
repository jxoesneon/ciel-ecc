import sys
import re

def process():
    with open('src/commands/mod.rs', 'r') as f:
        content = f.read()

    # 1. In commands/mod.rs, replace the module declarations and imports
    # with a `use crate::*;` and specific imports
    # We find where #[tokio::main] starts
    parts = content.split('#[tokio::main]\nasync fn main() -> Result<()> {\n')
    if len(parts) != 2:
        print("Could not find main function")
        return
    
    header = parts[0]
    body = parts[1]
    
    # In body, find where the match starts
    body_parts = body.split('match cli.command {\n')
    
    new_commands_rs = """use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Commands, MessageCommands, MessageKindArg, MigrationCommands, ObservationPriorityArg, RemoteCommands, ScheduleCommands, TaskPriorityArg, WorktreePolicyArgs, OptionalWorktreePolicyArgs, GraphCommands};
use crate::{comms, config, notifications, observability, session, tui, worktree};

""" + header.split('use cli::')[0].split('mod worktree;\nmod cli;\n#[cfg(test)]\nmod cli_tests;\n#[cfg(test)]\npub(crate) mod test_support;\n\n')[-1] + """

pub async fn execute(cli: Cli, cfg: config::Config, db: session::store::StateStore) -> Result<()> {
    match cli.command {
""" + body_parts[1]

    with open('src/commands/mod.rs', 'w') as f:
        f.write(new_commands_rs)

    # 2. Rewrite main.rs
    new_main_rs = """mod comms;
mod config;
mod notifications;
mod observability;
mod session;
mod tui;
mod worktree;
mod cli;
mod commands;

#[cfg(test)]
mod cli_tests;
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
"""
    with open('src/main.rs', 'w') as f:
        f.write(new_main_rs)

if __name__ == '__main__':
    process()
