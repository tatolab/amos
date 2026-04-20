use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;

use amos::adapter::AdapterRegistry;
use amos::adapter_pull;
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

    // Handle migrate command (runs before parsing into nodes — works on
    // legacy frontmatter that the parser might not accept cleanly).
    if let Some(Command::Migrate { dry_run }) = &cli.command {
        let report = amos::migrate::migrate_tree(&scan_root, *dry_run)
            .context("running migrate")?;
        print_migration_report(&report, *dry_run);
        return Ok(());
    }

    // Handle rename command. Runs on raw files — doesn't require parse.
    if let Some(Command::Rename { old_name, new_name, dry_run }) = &cli.command {
        let report = amos::rename::rename_tree(&scan_root, old_name, new_name, *dry_run)
            .context("running rename")?;
        if *dry_run {
            println!("# Rename dry-run (no files written)");
        } else {
            println!("# Rename report");
        }
        println!();
        println!("{}", report.summary());
        return Ok(());
    }

    // Status-mutation commands operate on `.amos-status` directly and don't
    // need a full scan/parse.
    match &cli.command {
        Some(Command::Done { node }) => {
            let mut sf = amos::status::StatusFile::load(&scan_root)?;
            sf.set(node, amos::status::Status::Done);
            sf.save()?;
            eprintln!("amos: marked '{}' done", node);
            return Ok(());
        }
        Some(Command::Start { node }) => {
            let mut sf = amos::status::StatusFile::load(&scan_root)?;
            sf.set(node, amos::status::Status::InProgress);
            sf.save()?;
            eprintln!("amos: marked '{}' in-progress", node);
            return Ok(());
        }
        Some(Command::Reset { node }) => {
            let mut sf = amos::status::StatusFile::load(&scan_root)?;
            sf.remove(node);
            sf.save()?;
            eprintln!("amos: reset '{}' to pending", node);
            return Ok(());
        }
        Some(Command::Focus { milestone, clear }) => {
            if *clear {
                amos::amosrc::write_focus(&scan_root, None)?;
                if cli.json {
                    println!("{}", serde_json::json!({"focus": null, "action": "cleared"}));
                } else {
                    eprintln!("amos: cleared focused milestone");
                }
            } else if let Some(m) = milestone {
                amos::amosrc::write_focus(&scan_root, Some(m))?;
                if cli.json {
                    println!("{}", serde_json::json!({"focus": m, "action": "set"}));
                } else {
                    eprintln!("amos: focused on milestone '{}'", m);
                }
            } else {
                let current = amos::amosrc::read_focus(&scan_root)?;
                if cli.json {
                    println!("{}", serde_json::json!({"focus": current}));
                } else {
                    match current {
                        Some(m) => println!("{}", m),
                        None => eprintln!("amos: no milestone currently focused"),
                    }
                }
            }
            return Ok(());
        }
        _ => {}
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
        print!("{}", output::format_graph(&dag, &registry, &scan_root));
        return Ok(());
    }

    // Handle show command
    if let Some(Command::Show { node }) = &cli.command {
        if cli.json {
            let Some(n) = dag.get_node(node) else {
                println!("{}", serde_json::json!({"error": "node not found", "name": node}));
                std::process::exit(1);
            };
            let adapter_fields = registry.resolve(node).and_then(|r| r.ok());
            let facts = adapter_fields
                .as_ref()
                .map(|f| &f.facts)
                .cloned()
                .unwrap_or_default();
            let body = adapter_fields.as_ref().and_then(|f| f.body.clone());
            println!("{}", serde_json::json!({
                "name": n.name,
                "description": n.description,
                "blocked_by": n.blocked_by,
                "blocks": n.blocks,
                "related_to": n.related_to,
                "duplicates": n.duplicates,
                "superseded_by": n.superseded_by,
                "labels_local": n.labels,
                "priority": n.priority.map(|p| format!("{:?}", p).to_lowercase()),
                "facts": facts,
                "body": body,
                "source_file": n.source_file.display().to_string(),
            }));
            return Ok(());
        }
        print!("{}", output::format_node(&dag, node, &registry));
        return Ok(());
    }

    // Handle validate command
    if matches!(&cli.command, Some(Command::Validate)) {
        let issues = dag.validate(&scan_root);
        if cli.json {
            let issues_json: Vec<serde_json::Value> = issues
                .iter()
                .map(|i| dag_issue_to_json(i))
                .collect();
            let result = serde_json::json!({
                "ok": issues.is_empty(),
                "node_count": dag.all_nodes().len(),
                "issues": issues_json,
            });
            println!("{}", result);
            if !issues.is_empty() {
                std::process::exit(1);
            }
            return Ok(());
        }
        if issues.is_empty() {
            eprintln!("amos: DAG clean — {} nodes, no issues", dag.all_nodes().len());
            return Ok(());
        }
        println!("## Issues");
        println!();
        for issue in &issues {
            println!("- {}", issue);
        }
        std::process::exit(1);
    }

    // Filter commands (next / blocked / orphans). All load the status file
    // so blocker completeness is computed against the user's local state.
    if matches!(
        &cli.command,
        Some(Command::Next) | Some(Command::Blocked) | Some(Command::Orphans)
    ) {
        let status_file = amos::status::StatusFile::load(&scan_root)?;
        let is_done = |name: &str| {
            status_file.get(name) == amos::status::Status::Done
        };

        // If a milestone is focused, pull adapter facts so we can scope
        // results to that milestone. Adapter state (closed/milestone) takes
        // precedence over .amos-status because the latter can drift.
        let focus = amos::amosrc::read_focus(&scan_root)?;
        let adapter_facts: std::collections::HashMap<String, amos::adapter::ResourceFields> =
            if focus.is_some() {
                let names: Vec<&str> = dag
                    .all_nodes()
                    .iter()
                    .filter(|n| registry.is_resolvable(&n.name))
                    .map(|n| n.name.as_str())
                    .collect();
                registry.resolve_batch(&names).unwrap_or_default()
            } else {
                std::collections::HashMap::new()
            };
        let in_focused_milestone = |name: &str| -> bool {
            let Some(ref focused) = focus else { return true };
            match adapter_facts.get(name) {
                Some(fields) => fields
                    .facts
                    .get("milestone")
                    .map(|m| m == focused)
                    .unwrap_or(false),
                None => false,
            }
        };
        let adapter_closed = |name: &str| -> bool {
            adapter_facts
                .get(name)
                .and_then(|f| f.facts.get("state"))
                .map(|s| s.eq_ignore_ascii_case("CLOSED"))
                .unwrap_or(false)
        };

        let mut matching: Vec<&amos::parser::Node> = Vec::new();
        for node in dag.all_nodes() {
            if !in_focused_milestone(&node.name) {
                continue;
            }
            // Trust adapter state over .amos-status when available.
            if adapter_closed(&node.name) {
                continue;
            }
            match &cli.command {
                Some(Command::Next) => {
                    if is_done(&node.name) {
                        continue;
                    }
                    let blockers = dag.blocked_by_of(&node.name);
                    let all_satisfied = blockers.iter().all(|b| {
                        is_done(&b.name) || adapter_closed(&b.name)
                    });
                    if all_satisfied {
                        matching.push(node);
                    }
                }
                Some(Command::Blocked) => {
                    if is_done(&node.name) {
                        continue;
                    }
                    let blockers = dag.blocked_by_of(&node.name);
                    let any_open = blockers.iter().any(|b| {
                        !is_done(&b.name) && !adapter_closed(&b.name)
                    });
                    if any_open {
                        matching.push(node);
                    }
                }
                Some(Command::Orphans) => {
                    if dag.blocked_by_of(&node.name).is_empty()
                        && dag.blocks_of(&node.name).is_empty()
                        && dag.related_of(&node.name).is_empty()
                    {
                        matching.push(node);
                    }
                }
                _ => {}
            }
        }

        matching.sort_by(|a, b| {
            amos::output::numeric_aware_cmp(&a.name, &b.name)
        });
        if cli.json {
            let arr: Vec<serde_json::Value> = matching
                .iter()
                .map(|n| node_to_json(n, adapter_facts.get(&n.name)))
                .collect();
            println!("{}", serde_json::json!({
                "focus": focus,
                "count": arr.len(),
                "nodes": arr,
            }));
            return Ok(());
        }
        for node in &matching {
            let desc = node.description.as_deref().unwrap_or("");
            if desc.is_empty() {
                println!("- {}", node.name);
            } else {
                println!("- {} — {}", node.name, desc);
            }
        }
        if matching.is_empty() {
            if let Some(ref f) = focus {
                eprintln!("amos: no matching nodes in focused milestone '{}'", f);
            } else {
                eprintln!("amos: no matching nodes");
            }
        }
        return Ok(());
    }

    // Milestones listing — per-milestone open/closed/ready counts pulled from
    // the adapter. "Ready" means an open node whose blocked_by set is fully
    // done (via .amos-status) or closed (via adapter state).
    if matches!(&cli.command, Some(Command::Milestones)) {
        let names: Vec<&str> = dag
            .all_nodes()
            .iter()
            .filter(|n| registry.is_resolvable(&n.name))
            .map(|n| n.name.as_str())
            .collect();
        let adapter_facts = registry.resolve_batch(&names).unwrap_or_default();
        let status_file = amos::status::StatusFile::load(&scan_root)?;
        let current_focus = amos::amosrc::read_focus(&scan_root)?;

        let is_closed = |node_name: &str| -> bool {
            adapter_facts
                .get(node_name)
                .and_then(|f| f.facts.get("state"))
                .map(|s| s.eq_ignore_ascii_case("CLOSED"))
                .unwrap_or(false)
        };
        let is_done_or_closed = |node_name: &str| -> bool {
            if is_closed(node_name) {
                return true;
            }
            status_file.get(node_name) == amos::status::Status::Done
        };

        // title -> (open, closed, ready)
        let mut milestone_counts: std::collections::BTreeMap<String, (usize, usize, usize)> =
            std::collections::BTreeMap::new();
        for (node_name, fields) in adapter_facts.iter() {
            let Some(ms) = fields.facts.get("milestone") else { continue };
            let state = fields
                .facts
                .get("state")
                .map(|s| s.as_str())
                .unwrap_or("OPEN");
            let entry = milestone_counts.entry(ms.clone()).or_default();
            if state.eq_ignore_ascii_case("CLOSED") {
                entry.1 += 1;
                continue;
            }
            entry.0 += 1;
            // Ready = all blockers satisfied.
            let blockers = dag.blocked_by_of(node_name);
            let all_satisfied = blockers.iter().all(|b| is_done_or_closed(&b.name));
            if all_satisfied {
                entry.2 += 1;
            }
        }
        if cli.json {
            let milestones_arr: Vec<serde_json::Value> = milestone_counts
                .iter()
                .map(|(title, (open, closed, ready))| {
                    serde_json::json!({
                        "title": title,
                        "open": open,
                        "ready": ready,
                        "done": closed,
                        "focused": current_focus.as_deref() == Some(title.as_str()),
                    })
                })
                .collect();
            println!("{}", serde_json::json!({
                "focus": current_focus,
                "milestones": milestones_arr,
            }));
            return Ok(());
        }
        if milestone_counts.is_empty() {
            eprintln!("amos: no milestones found in adapter data");
            return Ok(());
        }
        println!("{:2}{:50}  {:>6}  {:>6}  {:>6}", "", "milestone", "open", "ready", "done");
        for (title, (open, closed, ready)) in milestone_counts {
            let marker = if current_focus.as_deref() == Some(title.as_str()) {
                "* "
            } else {
                "  "
            };
            let label = format!("\"{}\"", title);
            println!(
                "{}{:50}  {:>6}  {:>6}  {:>6}",
                marker, label, open, ready, closed
            );
        }
        return Ok(());
    }

    // Default: dump the DAG
    eprintln!("amos: {} nodes", dag.all_nodes().len());
    print!("{}", output::format_dag(&dag, &registry));

    Ok(())
}

/// Serialize a DagIssue as a structured JSON object for `--json` output.
fn dag_issue_to_json(issue: &amos::dag::DagIssue) -> serde_json::Value {
    use amos::dag::DagIssue;
    match issue {
        DagIssue::DuplicateName { name, files } => serde_json::json!({
            "kind": "duplicate_name",
            "name": name,
            "files": files,
        }),
        DagIssue::MissingDependency { from_node, missing_dep } => serde_json::json!({
            "kind": "missing_dependency",
            "from": from_node,
            "missing": missing_dep,
        }),
        DagIssue::CycleDetected => serde_json::json!({
            "kind": "cycle_detected",
        }),
        DagIssue::DanglingContext { node, context_path } => serde_json::json!({
            "kind": "dangling_context",
            "node": node,
            "context_path": context_path,
        }),
    }
}

/// Serialize a Node (plus optional adapter facts) for JSON output of
/// `next` / `blocked` / `orphans`. Keeps the payload compact — full body
/// is only returned by `show --json`.
fn node_to_json(
    node: &amos::parser::Node,
    facts: Option<&amos::adapter::ResourceFields>,
) -> serde_json::Value {
    let issue_number = node
        .name
        .rsplit_once('#')
        .and_then(|(_, n)| n.parse::<u64>().ok());
    serde_json::json!({
        "name": node.name,
        "issue_number": issue_number,
        "description": node.description,
        "blocked_by": node.blocked_by,
        "blocks": node.blocks,
        "related_to": node.related_to,
        "labels_local": node.labels,
        "facts": facts.map(|f| f.facts.clone()).unwrap_or_default(),
        "milestone": facts.and_then(|f| f.facts.get("milestone").cloned()),
    })
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
    let pulled = adapter_pull::build_declared_adapters(nodes);
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

/// Print a human-readable summary of a migration run.
fn print_migration_report(report: &amos::migrate::MigrationReport, dry_run: bool) {
    use amos::migrate::FileChange;

    if dry_run {
        println!("# Migration dry-run (no files written)");
    } else {
        println!("# Migration report");
    }
    println!();
    println!("{}", report.summary());
    println!();

    let mut migrated: Vec<&amos::migrate::FileReport> = report
        .files
        .iter()
        .filter(|r| r.kind == FileChange::Migrated)
        .collect();
    migrated.sort_by(|a, b| a.path.cmp(&b.path));

    if migrated.is_empty() {
        println!("No files needed migration.");
        return;
    }

    println!("## Files migrated");
    println!();
    for r in &migrated {
        let status_note = match r.status_moved {
            Some(s) => format!(" status→.amos-status({:?})", s),
            None => String::new(),
        };
        println!(
            "- {}  (+{} blocked_by, +{} blocks{})",
            r.path.display(),
            r.blocked_by_added,
            r.blocks_added,
            status_note
        );
    }
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
