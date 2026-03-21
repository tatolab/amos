use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

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

    // Scan + parse
    let blocks = scanner::scan_directory(&scan_root)
        .with_context(|| format!("scanning {}", scan_root.display()))?;

    if blocks.is_empty() {
        eprintln!("No amos blocks found in {}", scan_root.display());
        std::process::exit(1);
    }

    let nodes = parser::parse_blocks(&blocks).context("parsing amos blocks")?;

    // Build adapter registry
    let registry = build_registry(&scan_root, &nodes);

    // Build DAG
    let dag = Dag::build(nodes.clone()).context("building DAG")?;

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

    // Default: dump the DAG
    eprintln!("amos: {} nodes", dag.all_nodes().len());
    print!("{}", output::format_dag(&dag, &registry));

    Ok(())
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
