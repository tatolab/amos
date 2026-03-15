use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "amos",
    about = "Scans markdown files for work stream DAG blocks, builds dependency graphs, reports what he sees",
    version
)]
pub struct Cli {
    /// Override scan root directory (default: cwd)
    #[arg(long, global = true)]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Mark a node as done
    Done {
        /// Node name
        node: String,
    },
    /// Mark a node as in-progress
    Start {
        /// Node name
        node: String,
    },
    /// Clear a node's status
    Reset {
        /// Node name
        node: String,
    },
    /// Sync status from external adapters (resolves URIs in node names)
    Sync,
}
