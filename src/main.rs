use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use std::collections::HashSet;

use amos::adapter::AdapterRegistry;
use amos::adapter_pull::{self, TrustConfig};
use amos::cli::{Cli, Command};
use amos::dag::Dag;
use amos::ffmpeg_adapter::FfmpegAdapter;
use amos::file_adapter::FileAdapter;
use amos::gh_adapter::GhAdapter;
use amos::url_adapter::UrlAdapter;
use amos::output;
use amos::parser;
use amos::scanner;
use amos::status;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let scan_root = cli
        .dir
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let scan_root = scan_root
        .canonicalize()
        .with_context(|| format!("resolving scan root: {}", scan_root.display()))?;

    // Handle notify command (no scan needed)
    if let Some(Command::Notify { node, message }) = &cli.command {
        let registry = build_registry_minimal(&scan_root);
        registry.notify(node, message);
        return Ok(());
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
    let registry = build_registry(&scan_root, &nodes);

    // Handle commands that need the DAG
    let mut dag = Dag::build(nodes.clone()).context("building DAG")?;
    let statuses = status::read_status_file(&scan_root);
    dag.apply_status_overlay(statuses);

    // Handle prune command
    if matches!(&cli.command, Some(Command::Prune)) {
        return handle_prune(&dag, &scan_root);
    }

    // Handle graph command
    if matches!(&cli.command, Some(Command::Graph)) {
        print!("{}", output::format_graph(&dag));
        return Ok(());
    }

    // Handle show command
    if let Some(Command::Show { node }) = &cli.command {
        print!("{}", output::format_node(&dag, node, &registry));
        return Ok(());
    }

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
                status::write_status(&scan_root, uri, s)
                    .with_context(|| format!("writing status for {}", uri))?;
                eprintln!("[{}] {}", s, uri);
                synced += 1;
            }
        }

        eprintln!("amos: synced {} node(s)", synced);
        return Ok(());
    }

    // Default: dump the DAG
    eprintln!("amos: {} nodes", dag.all_nodes().len());
    print!("{}", output::format_dag(&dag, &registry));

    Ok(())
}

/// Prune done nodes that aren't upstream of any ready/in-progress work.
fn handle_prune(dag: &Dag, scan_root: &std::path::Path) -> Result<()> {
    // Find all actionable nodes (ready or in-progress)
    let active_nodes: Vec<&str> = dag
        .all_nodes()
        .iter()
        .filter(|n| {
            dag.compute_status(&n.name)
                .map_or(false, |s| s.is_actionable())
        })
        .map(|n| n.name.as_str())
        .collect();

    // Collect all nodes that are transitively upstream of active nodes
    let mut needed: HashSet<String> = HashSet::new();
    for name in &active_nodes {
        needed.insert(name.to_string());
        collect_upstream(dag, name, &mut needed);
    }

    // Also keep non-done nodes (they have pending work)
    for node in dag.all_nodes() {
        if !dag
            .compute_status(&node.name)
            .map_or(false, |s| s.is_done())
        {
            needed.insert(node.name.clone());
        }
    }

    // Find done nodes that aren't needed
    let all_nodes = dag.all_nodes();
    let prunable: Vec<_> = all_nodes
        .iter()
        .filter(|n| {
            dag.compute_status(&n.name)
                .map_or(false, |s| s.is_done())
                && !needed.contains(&n.name)
        })
        .collect();

    if prunable.is_empty() {
        eprintln!("amos: nothing to prune");
        return Ok(());
    }

    for node in &prunable {
        // Delete the source file
        if node.source_file.exists() {
            std::fs::remove_file(&node.source_file).with_context(|| {
                format!("deleting {}", node.source_file.display())
            })?;
            eprintln!("pruned: {} ({})", node.name, node.source_file.display());
        }

        // Remove from .amos-status
        let _ = status::clear_status(scan_root, &node.name);
    }

    eprintln!("amos: pruned {} node(s)", prunable.len());
    Ok(())
}

/// Recursively collect all upstream nodes.
fn collect_upstream(dag: &Dag, name: &str, visited: &mut HashSet<String>) {
    for upstream in dag.upstream_of(name) {
        if visited.insert(upstream.name.clone()) {
            collect_upstream(dag, &upstream.name, visited);
        }
    }
}

/// Build the adapter registry.
///
/// Priority (last wins): built-in → auto-pulled from frontmatter → .amosrc.toml
/// This means frontmatter can override built-ins, and .amosrc.toml overrides everything.
fn build_registry(scan_root: &std::path::Path, nodes: &[amos::parser::Node]) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();

    // 1. Built-in adapters (defaults, can be overridden)
    registry.register(Box::new(FileAdapter::new(scan_root)));
    registry.register(Box::new(GhAdapter::new(None)));
    registry.register(Box::new(UrlAdapter::new()));
    registry.register(Box::new(FfmpegAdapter::new(scan_root)));

    // 2. Auto-pull adapters from frontmatter declarations
    //    "builtin" sources are skipped. Custom sources override built-ins.
    let trust = TrustConfig::load(scan_root);
    let pulled = adapter_pull::build_declared_adapters(nodes, &trust);
    for (_scheme, adapter) in pulled {
        registry.register(Box::new(adapter));
    }

    // 3. Local config overrides (.amosrc.toml [adapters] section)
    let config_path = scan_root.join(".amosrc.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = content.parse::<toml::Table>() {
                if let Some(adapters) = config.get("adapters").and_then(|v| v.as_table()) {
                    for (scheme, settings) in adapters {
                        if let Some(command) = settings.get("command").and_then(|v| v.as_str()) {
                            eprintln!("amos: registered external adapter '{}' → {}", scheme, command);
                            registry.register(Box::new(amos::external_adapter::ExternalAdapter::new(scheme, command)));
                        }
                    }
                }
            }
        }
    }

    registry
}

/// Build a minimal registry with just built-in adapters.
/// Used for status notifications where we don't need to scan/parse nodes.
fn build_registry_minimal(scan_root: &std::path::Path) -> AdapterRegistry {
    let mut registry = AdapterRegistry::new();
    registry.register(Box::new(FileAdapter::new(scan_root)));
    registry.register(Box::new(GhAdapter::new(None)));
    registry.register(Box::new(UrlAdapter::new()));
    registry.register(Box::new(FfmpegAdapter::new(scan_root)));
    registry
}
