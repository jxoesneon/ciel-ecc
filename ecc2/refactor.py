import re

def main():
    with open('src/main.rs', 'r') as f:
        content = f.read()

    # 1. Extract test_support
    test_support_pattern = re.compile(r'#\[cfg\(test\)\]\npub\(crate\) mod test_support \{(.*?)\n\}\n', re.DOTALL)
    test_support_match = test_support_pattern.search(content)
    if test_support_match:
        with open('src/test_support.rs', 'w') as f:
            f.write(test_support_match.group(1).strip() + '\n')
        content = content[:test_support_match.start()] + content[test_support_match.end():]

    # 2. Extract tests
    test_start = content.find('#[cfg(test)]\nmod tests {')
    if test_start != -1:
        test_content = content[test_start:]
        # Remove the `#[cfg(test)]\nmod tests {` wrapper and the last `}`
        # Actually, let's keep the `#[cfg(test)]\nmod tests {` wrapper in the file for simplicity, 
        # but change `use super::*;` to `use crate::*;` maybe?
        # Actually, just writing the whole test module block to `cli_tests.rs` is fine.
        with open('src/cli_tests.rs', 'w') as f:
            f.write(test_content)
        content = content[:test_start]
        
        # fix super::* in cli_tests.rs
        with open('src/cli_tests.rs', 'r') as f:
            tc = f.read()
        tc = tc.replace('use super::*;', 'use crate::*;')
        with open('src/cli_tests.rs', 'w') as f:
            f.write(tc)

    # 3. Extract CLI definitions
    cli_start = content.find('#[derive(Parser, Debug)]')
    # The CLI definitions end right before `#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]\nstruct GraphConnectorSyncStats`
    cli_end = content.find('#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]\nstruct GraphConnectorSyncStats')
    
    if cli_start != -1 and cli_end != -1:
        cli_content = content[cli_start:cli_end]
        with open('src/cli.rs', 'w') as f:
            f.write("""use clap::Parser;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::config;
use crate::comms;
use crate::session;

""" + cli_content)
        content = content[:cli_start] + content[cli_end:]

    # 4. Extract main and commands logic
    # Find `async fn main()`
    main_start = content.find('#[tokio::main]\nasync fn main() -> Result<()> {')
    if main_start != -1:
        header = content[:main_start]
        body = content[main_start:]
        
        # In body, find `match cli.command {`
        match_start = body.find('match cli.command {\n')
        match_end = body.find('    Ok(())\n}')
        
        if match_start != -1 and match_end != -1:
            match_block = body[match_start:match_end]
            helper_functions = body[match_end + 12:] # everything after `Ok(())\n}`
            
            # Write commands/mod.rs
            with open('src/commands/mod.rs', 'w') as f:
                f.write("""use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, Commands, MessageCommands, MessageKindArg, MigrationCommands, ObservationPriorityArg, RemoteCommands, ScheduleCommands, TaskPriorityArg, WorktreePolicyArgs, OptionalWorktreePolicyArgs, GraphCommands};
use crate::{comms, config, notifications, observability, session, tui, worktree};

""" + header.split('use tracing_subscriber::EnvFilter;\n')[-1] + """

pub async fn execute(cli: Cli, cfg: config::Config, db: session::store::StateStore) -> Result<()> {
    """ + match_block + """    Ok(())
}

""" + helper_functions)

    # 5. Write thin main.rs
    with open('src/main.rs', 'w') as f:
        f.write("""mod comms;
mod config;
mod notifications;
mod observability;
mod session;
mod tui;
mod worktree;
pub mod cli;
pub mod commands;

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
""")

if __name__ == '__main__':
    main()
