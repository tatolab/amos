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
    /// Send a message to a node's source adapter (comment, log, status update)
    Notify {
        /// Node name or URI (e.g. @github:tatolab/amos#22)
        node: String,
        /// Freeform message — the adapter decides how to record it
        message: String,
    },
    /// Sync status from external adapters (resolves URIs in node names)
    Sync,
    /// Remove done nodes that aren't needed by active work
    Prune,
    /// Print the DAG as an ASCII dependency tree
    Graph,
    /// Show a single node with its fully resolved body
    Show {
        /// Node name (e.g. @github:tatolab/amos#22)
        node: String,
    },
}
