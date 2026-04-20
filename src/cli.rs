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
    /// Print the DAG as an ASCII dependency tree
    Graph,
    /// Show a single node with its fully resolved body
    Show {
        /// Node name (e.g. @github:tatolab/amos#22)
        node: String,
    },
    /// Migrate legacy `dependencies: up:/down:` frontmatter to the typed
    /// relationship model (`blocked_by:`/`blocks:`). Status fields move to
    /// `.amos-status`. Grouping is intentionally not handled — it's delegated
    /// to GitHub milestones, resolved through the adapter.
    Migrate {
        /// Show the summary without writing files or the status file.
        #[arg(long)]
        dry_run: bool,
    },
    /// Mark a node as done in `.amos-status`.
    Done {
        /// Node name (e.g. @github:tatolab/amos#22)
        node: String,
    },
    /// Mark a node as in-progress in `.amos-status`.
    Start {
        /// Node name
        node: String,
    },
    /// Clear a node's `.amos-status` entry (back to pending).
    Reset {
        /// Node name
        node: String,
    },
    /// Run DAG integrity checks — missing deps, cycles, asymmetric edges.
    /// Exits non-zero when errors are present.
    Validate,
    /// Print nodes that are ready to start — all blockers done, node itself
    /// not yet done.
    Next,
    /// Print nodes that are blocked — have at least one non-done blocker.
    Blocked,
    /// Print nodes with no relationships at all — no blocks/blocked_by/related_to.
    Orphans,
    /// Set the focused milestone in `.amosrc.toml`. Subsequent `amos next`,
    /// `amos blocked`, `amos orphans`, and `amos graph` calls scope their
    /// results to this milestone. Pass `--clear` to remove the focus.
    Focus {
        /// Milestone title (must match the GitHub milestone exactly).
        #[arg(conflicts_with = "clear")]
        milestone: Option<String>,
        /// Clear the current focus, returning to unfiltered behaviour.
        #[arg(long)]
        clear: bool,
    },
    /// List milestones for every focus-capable node in the scan, pulled from
    /// the adapter. Useful when deciding what to `amos focus` on.
    Milestones,
    /// Rename a node and every reference to it across all scanned files.
    /// Use this instead of hand-editing — stale references silently break DAG
    /// edges because the string no longer matches.
    Rename {
        /// Current node name (full canonical form).
        old_name: String,
        /// New node name (full canonical form).
        new_name: String,
        /// Show the summary without writing files.
        #[arg(long)]
        dry_run: bool,
    },
}
