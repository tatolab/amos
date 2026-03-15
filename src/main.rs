use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use amos::adapter::AdapterRegistry;
use amos::cli::{Cli, Command};
use amos::dag::Dag;
use amos::file_adapter::FileAdapter;
use amos::gh_adapter::GhAdapter;
use amos::url_adapter::UrlAdapter;
use amos::output;
use amos::parser;
use amos::scanner;
use amos::status::{self, ManualStatus};

fn main() -> Result<()> {
    let cli = Cli::parse();

    let scan_root = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let scan_root = scan_root
        .canonicalize()
        .with_context(|| format!("resolving scan root: {}", scan_root.display()))?;

    // Handle status mutation commands (no scan needed)
    match &cli.command {
        Some(Command::Done { node }) => {
            status::write_status(&scan_root, node, ManualStatus::Done)
                .with_context(|| format!("marking {} done", node))?;
            eprintln!("[x] {}", node);
            return Ok(());
        }
        Some(Command::Start { node }) => {
            status::write_status(&scan_root, node, ManualStatus::InProgress)
                .with_context(|| format!("starting {}", node))?;
            eprintln!("[~] {}", node);
            return Ok(());
        }
        Some(Command::Reset { node }) => {
            status::clear_status(&scan_root, node)
                .with_context(|| format!("resetting {}", node))?;
            eprintln!("[ ] {}", node);
            return Ok(());
        }
        _ => {}
    }

    // Scan + parse (needed for sync and default dump)
    let blocks = scanner::scan_directory(&scan_root)
        .with_context(|| format!("scanning {}", scan_root.display()))?;

    if blocks.is_empty() {
        eprintln!("No amos blocks found in {}", scan_root.display());
        std::process::exit(1);
    }

    let nodes = parser::parse_blocks(&blocks).context("parsing amos blocks")?;

    // Build adapter registry
    let registry = build_registry(&scan_root);

    // Handle sync command
    if matches!(&cli.command, Some(Command::Sync)) {
        let uri_nodes: Vec<&str> = nodes
            .iter()
            .filter(|n| registry.is_resolvable(&n.name))
            .map(|n| n.name.as_str())
            .collect();

        if uri_nodes.is_empty() {
            eprintln!("No adapter-backed nodes found");
            return Ok(());
        }

        let results = registry
            .resolve_batch(&uri_nodes)
            .context("syncing from adapters")?;

        let mut synced = 0;
        for (uri, fields) in &results {
            if let Some(s) = &fields.status {
                status::write_status(&scan_root, uri, *s)
                    .with_context(|| format!("writing status for {}", uri))?;
                let symbol = match s {
                    ManualStatus::Done => "x",
                    ManualStatus::InProgress => "~",
                };
                eprintln!("[{}] {}", symbol, uri);
                synced += 1;
            }
        }

        eprintln!("amos: synced {} node(s)", synced);
        return Ok(());
    }

    // Default: dump the DAG
    let mut dag = Dag::build(nodes).context("building DAG")?;

    let statuses = status::read_status_file(&scan_root);
    dag.apply_status_overlay(statuses);

    eprintln!("amos: {} nodes", dag.all_nodes().len());
    print!("{}", output::format_dag(&dag, &registry));

    Ok(())
}

/// Build the adapter registry with built-in adapters.
fn build_registry(scan_root: &std::path::Path) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();

    // Built-in: file adapter (always available)
    registry.register(Box::new(FileAdapter::new(scan_root)));

    // Built-in: gh adapter (uses gh CLI at runtime, handles private repos)
    registry.register(Box::new(GhAdapter::new(None)));

    // Built-in: url adapter (downloads public URLs to local cache)
    registry.register(Box::new(UrlAdapter::new()));

    registry
}
