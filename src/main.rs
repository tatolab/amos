use anyhow::{bail, Context, Result};
use clap::Parser;
use std::collections::HashSet;
use std::path::PathBuf;

use amos::adapter::{AdapterNode, AdapterRegistry, IssueSpec, RelationshipKind};
use amos::adapter_pull;
use amos::cli::{Cli, Command};
use amos::dag::Dag;
use amos::ffmpeg_adapter::FfmpegAdapter;
use amos::file_adapter::FileAdapter;
use amos::gh_adapter::GhAdapter;
use amos::url_adapter::UrlAdapter;
use amos::output;
use amos::parser::{self, Node};
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

    // Issue create talks to the adapter only — no local scan required. Route
    // it early so projects with legacy plan-file formats can still file new
    // issues without having to migrate first.
    if let Some(Command::IssueCreate { scheme, spec }) = &cli.command {
        let registry = build_registry(&scan_root, &[]);
        return run_issue_create(&registry, scheme, spec, cli.json);
    }

    // Scan + parse. With adapter-first enumeration, most queries (graph,
    // next, blocked, orphans, milestones) pull their node set from the
    // adapter when a milestone is focused — a project can be fully
    // remote-first with zero local plan files and still be usable.
    let blocks = scanner::scan_directory(&scan_root)
        .with_context(|| format!("scanning {}", scan_root.display()))?;

    // Only commands that operate purely on local files (`validate`, the DAG
    // dump that runs by default) require at least one local block. Every
    // other command either reads from the adapter or reshapes state that
    // exists independently of plan files.
    let requires_local_blocks = matches!(
        &cli.command,
        Some(Command::Validate) | None
    );
    if blocks.is_empty() && requires_local_blocks {
        eprintln!("No amos blocks found in {}", scan_root.display());
        std::process::exit(1);
    }

    let local_nodes = parser::parse_blocks(&blocks).context("parsing amos blocks")?;

    // Build adapter registry
    let registry = build_registry(&scan_root, &local_nodes);

    // If a milestone is focused, enumerate every issue in it from the adapter
    // and merge with local plan files. This is what makes unplanned GitHub
    // issues visible to `graph` / `next` / `blocked` — they don't need a
    // plan file to show up. Native relationship edges (GitHub's typed
    // blockedBy / blocking / parent / subIssues) are also merged.
    let focus = amos::amosrc::read_focus(&scan_root)?;
    let mut adapter_nodes_by_name: std::collections::HashMap<String, AdapterNode> =
        std::collections::HashMap::new();
    let nodes = if let Some(focused) = focus.as_deref() {
        let adapter_nodes = registry.list_nodes_in_milestone(focused);
        for an in &adapter_nodes {
            adapter_nodes_by_name.insert(an.name.clone(), an.clone());
        }
        merge_local_and_adapter_nodes(local_nodes, adapter_nodes)
    } else {
        local_nodes
    };

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

        // `focus` was read once up top (driving the adapter enumeration).
        // Pull adapter facts for every DAG node so blocker state checks can
        // consult authoritative adapter state, not just local `.amos-status`.
        // resolve_batch is a single paginated call for github, so it's cheap.
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

    // Milestones listing — source of truth is the adapter, not the local DAG.
    // `open`/`done` come directly from native milestone counts; `ready` is
    // computed per-milestone by enumerating issues and resolving each
    // blocker's state.
    if matches!(&cli.command, Some(Command::Milestones)) {
        let status_file = amos::status::StatusFile::load(&scan_root)?;
        let current_focus = amos::amosrc::read_focus(&scan_root)?;

        let mut milestones = registry.list_all_milestones();
        milestones.sort_by(|a, b| a.title.cmp(&b.title));

        // For every open milestone, enumerate its nodes (issues + native
        // relationships) so we can compute `ready`. Collect blocker names
        // that point outside the milestone — we resolve their state in a
        // single batch call at the end.
        let mut by_milestone: std::collections::BTreeMap<String, Vec<AdapterNode>> =
            std::collections::BTreeMap::new();
        let mut extra_state_lookups: HashSet<String> = HashSet::new();
        for ms in &milestones {
            if ms.open_count == 0 {
                by_milestone.insert(ms.title.clone(), Vec::new());
                continue;
            }
            let nodes = registry.list_nodes_in_milestone(&ms.title);
            let in_ms: HashSet<String> = nodes.iter().map(|n| n.name.clone()).collect();
            for n in &nodes {
                for blocker in &n.blocked_by {
                    if !in_ms.contains(blocker) {
                        extra_state_lookups.insert(blocker.clone());
                    }
                }
            }
            by_milestone.insert(ms.title.clone(), nodes);
        }

        let extra_refs: Vec<&str> = extra_state_lookups.iter().map(|s| s.as_str()).collect();
        let extra_facts = registry.resolve_batch(&extra_refs).unwrap_or_default();

        let is_closed_name = |name: &str, within: &[AdapterNode]| -> bool {
            if let Some(n) = within.iter().find(|n| n.name == name) {
                return n
                    .facts
                    .get("state")
                    .map(|s| s.eq_ignore_ascii_case("CLOSED"))
                    .unwrap_or(false);
            }
            if let Some(f) = extra_facts.get(name) {
                if let Some(state) = f.facts.get("state") {
                    return state.eq_ignore_ascii_case("CLOSED");
                }
            }
            status_file.get(name) == amos::status::Status::Done
        };

        if cli.json {
            let arr: Vec<serde_json::Value> = milestones
                .iter()
                .map(|ms| {
                    let nodes = by_milestone
                        .get(&ms.title)
                        .cloned()
                        .unwrap_or_default();
                    let ready = count_ready_in(&nodes, &|n| is_closed_name(n, &nodes));
                    serde_json::json!({
                        "title": ms.title,
                        "state": ms.state,
                        "open": ms.open_count,
                        "done": ms.closed_count,
                        "ready": ready,
                        "focused": current_focus.as_deref() == Some(ms.title.as_str()),
                    })
                })
                .collect();
            println!("{}", serde_json::json!({
                "focus": current_focus,
                "milestones": arr,
            }));
            return Ok(());
        }

        if milestones.is_empty() {
            eprintln!("amos: no milestones found");
            return Ok(());
        }
        println!("{:2}{:50}  {:>6}  {:>6}  {:>6}", "", "milestone", "open", "ready", "done");
        for ms in &milestones {
            let nodes = by_milestone.get(&ms.title).cloned().unwrap_or_default();
            let ready = count_ready_in(&nodes, &|n| is_closed_name(n, &nodes));
            let marker = if current_focus.as_deref() == Some(ms.title.as_str()) {
                "* "
            } else {
                "  "
            };
            let label = format!("\"{}\"", ms.title);
            println!(
                "{}{:50}  {:>6}  {:>6}  {:>6}",
                marker, label, ms.open_count, ready, ms.closed_count
            );
        }
        return Ok(());
    }

    // Sync edges — one-time migration from local plan-file edges to native
    // adapter relationships (GitHub's typed issue dependencies). Idempotent.
    if let Some(Command::SyncEdges { dry_run }) = &cli.command {
        return run_sync_edges(&registry, &nodes, *dry_run, cli.json);
    }

    // Issue create — atomic create-with-relationships from a JSON spec.
    if let Some(Command::IssueCreate { scheme, spec }) = &cli.command {
        return run_issue_create(&registry, scheme, spec, cli.json);
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
    registry.register(Box::new(GhAdapter::with_detected_repo(scan_root)));
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
    registry.register(Box::new(GhAdapter::with_detected_repo(scan_root)));
    registry.register(Box::new(UrlAdapter::new()));
    registry.register(Box::new(FfmpegAdapter::new(scan_root)));
    registry
}

/// Merge adapter-sourced nodes with locally-parsed nodes. Local nodes win for
/// every AI-facing field (body, context, labels, description) — the adapter
/// just enriches with native edges. Adapter nodes that have no local match
/// are added as virtual nodes so the DAG can see them.
fn merge_local_and_adapter_nodes(
    mut local: Vec<Node>,
    adapter_nodes: Vec<AdapterNode>,
) -> Vec<Node> {
    let local_names: HashSet<String> = local.iter().map(|n| n.name.clone()).collect();

    for an in &adapter_nodes {
        if let Some(existing) = local.iter_mut().find(|n| n.name == an.name) {
            for blocker in &an.blocked_by {
                if !existing.blocked_by.contains(blocker) {
                    existing.blocked_by.push(blocker.clone());
                }
            }
            for blocked in &an.blocks {
                if !existing.blocks.contains(blocked) {
                    existing.blocks.push(blocked.clone());
                }
            }
        }
    }

    for an in adapter_nodes {
        if local_names.contains(&an.name) {
            continue;
        }
        local.push(virtual_node_from_adapter(an));
    }

    local
}

/// Construct a minimal `Node` from an `AdapterNode` so it can participate in
/// the DAG without a local plan file. `source_file` is a sentinel path.
fn virtual_node_from_adapter(an: AdapterNode) -> Node {
    let labels = an
        .facts
        .get("labels")
        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
        .unwrap_or_default();
    Node {
        name: an.name,
        description: if an.title.is_empty() { None } else { Some(an.title) },
        blocked_by: an.blocked_by,
        blocks: an.blocks,
        related_to: Vec::new(),
        duplicates: None,
        superseded_by: None,
        labels,
        priority: None,
        context: Vec::new(),
        adapters: std::collections::HashMap::new(),
        source_file: PathBuf::from("<adapter>"),
        line_number: 0,
        body: String::new(),
    }
}

/// Count adapter nodes that are open AND have zero open blockers.
fn count_ready_in(
    nodes: &[AdapterNode],
    is_closed: &dyn Fn(&str) -> bool,
) -> usize {
    nodes
        .iter()
        .filter(|n| {
            let state = n.facts.get("state").map(|s| s.as_str()).unwrap_or("OPEN");
            if state.eq_ignore_ascii_case("CLOSED") {
                return false;
            }
            n.blocked_by.iter().all(|b| is_closed(b))
        })
        .count()
}

/// Create a new issue from a JSON spec, then atomically apply native
/// relationship edges. Returns the URL + canonical amos name so a calling
/// skill can reference the newly-created issue immediately.
fn run_issue_create(
    registry: &AdapterRegistry,
    scheme: &str,
    spec_path: &str,
    as_json: bool,
) -> Result<()> {
    let raw = if spec_path == "-" {
        let mut buf = String::new();
        use std::io::Read;
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("reading spec from stdin")?;
        buf
    } else {
        std::fs::read_to_string(spec_path)
            .with_context(|| format!("reading spec from {}", spec_path))?
    };

    let parsed: serde_json::Value = serde_json::from_str(&raw)
        .context("parsing issue spec JSON")?;

    let spec = IssueSpec {
        title: parsed
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        body: parsed
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        milestone: parsed
            .get("milestone")
            .and_then(|v| v.as_str())
            .map(String::from),
        labels: json_string_array(&parsed, "labels"),
        issue_type: parsed
            .get("issue_type")
            .and_then(|v| v.as_str())
            .map(String::from),
        blocked_by: json_string_array(&parsed, "blocked_by"),
        blocks: json_string_array(&parsed, "blocks"),
        sub_issue_of: parsed
            .get("sub_issue_of")
            .and_then(|v| v.as_str())
            .map(String::from),
    };

    let created = registry
        .create_issue(scheme, &spec)
        .context("creating issue")?;

    // Apply relationships. Failures don't undo the issue creation (the
    // issue is already there); we report them in the result so the caller
    // can decide whether to retry.
    let mut relationship_failures: Vec<(String, String, String)> = Vec::new();
    for blocker in &spec.blocked_by {
        if let Err(e) = registry.add_relationship(&created.name, blocker, RelationshipKind::BlockedBy) {
            relationship_failures.push((
                created.name.clone(),
                blocker.clone(),
                format!("{:#}", e),
            ));
        }
    }
    for blocked in &spec.blocks {
        if let Err(e) = registry.add_relationship(&created.name, blocked, RelationshipKind::Blocks) {
            relationship_failures.push((
                created.name.clone(),
                blocked.clone(),
                format!("{:#}", e),
            ));
        }
    }
    if let Some(parent) = &spec.sub_issue_of {
        if let Err(e) = registry.add_relationship(&created.name, parent, RelationshipKind::SubIssueOf) {
            relationship_failures.push((
                created.name.clone(),
                parent.clone(),
                format!("{:#}", e),
            ));
        }
    }

    if as_json {
        let failures_json: Vec<serde_json::Value> = relationship_failures
            .iter()
            .map(|(f, t, e)| serde_json::json!({"from": f, "to": t, "error": e}))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "name": created.name,
                "number": created.number,
                "url": created.url,
                "relationship_failures": failures_json,
            })
        );
    } else {
        println!("amos: created {} — {}", created.name, created.url);
        if !relationship_failures.is_empty() {
            eprintln!("amos: {} relationship(s) failed to apply:", relationship_failures.len());
            for (f, t, e) in &relationship_failures {
                eprintln!("  {} → {}: {}", f, t, e);
            }
        }
    }
    Ok(())
}

fn json_string_array(value: &serde_json::Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Push every local `blocked_by` / `blocks` edge up to the adapter as a
/// native relationship. Each operation is idempotent — "already exists"
/// responses are logged as success.
fn run_sync_edges(
    registry: &AdapterRegistry,
    nodes: &[Node],
    dry_run: bool,
    as_json: bool,
) -> Result<()> {
    // Collect edges as a canonical (blocked, blocker) pair so mirror
    // declarations (plan A says `blocks: [B]` AND plan B says `blocked_by:
    // [A]`) deduplicate before we push them to the adapter. We emit every
    // edge as the BlockedBy variant because that's what the GitHub mutation
    // takes directly.
    let mut edge_set: std::collections::BTreeSet<(String, String)> = Default::default();
    for node in nodes {
        if !registry.is_resolvable(&node.name) {
            continue;
        }
        for blocker in &node.blocked_by {
            if registry.is_resolvable(blocker) {
                edge_set.insert((node.name.clone(), blocker.clone()));
            }
        }
        for blocked in &node.blocks {
            if registry.is_resolvable(blocked) {
                edge_set.insert((blocked.clone(), node.name.clone()));
            }
        }
    }
    let planned: Vec<(String, String, RelationshipKind)> = edge_set
        .into_iter()
        .map(|(blocked, blocker)| (blocked, blocker, RelationshipKind::BlockedBy))
        .collect();

    if planned.is_empty() {
        if as_json {
            println!(
                "{}",
                serde_json::json!({"planned": 0, "applied": 0, "dry_run": dry_run, "edges": []})
            );
        } else {
            eprintln!("amos: no local edges to sync");
        }
        return Ok(());
    }

    let edges_json: Vec<serde_json::Value> = planned
        .iter()
        .map(|(from, to, kind)| {
            serde_json::json!({
                "from": from,
                "to": to,
                "kind": format!("{:?}", kind),
            })
        })
        .collect();

    let mut applied = 0usize;
    let mut failed: Vec<(String, String, String)> = Vec::new();

    for (from, to, kind) in &planned {
        if dry_run {
            if !as_json {
                println!("- would sync: {} {:?} {}", from, kind, to);
            }
            continue;
        }
        match registry.add_relationship(from, to, *kind) {
            Ok(()) => {
                applied += 1;
                if !as_json {
                    println!("✓ {} {:?} {}", from, kind, to);
                }
            }
            Err(e) => {
                let msg = format!("{:#}", e);
                let lower = msg.to_ascii_lowercase();
                if lower.contains("already") || lower.contains("duplicate") {
                    applied += 1;
                    if !as_json {
                        println!("• {} {:?} {} (already exists)", from, kind, to);
                    }
                } else {
                    failed.push((from.clone(), to.clone(), msg.clone()));
                    if !as_json {
                        eprintln!("✗ {} {:?} {}: {}", from, kind, to, msg);
                    }
                }
            }
        }
    }

    if as_json {
        let failures: Vec<serde_json::Value> = failed
            .iter()
            .map(|(from, to, err)| serde_json::json!({"from": from, "to": to, "error": err}))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "planned": planned.len(),
                "applied": applied,
                "failed": failures.len(),
                "failures": failures,
                "dry_run": dry_run,
                "edges": edges_json,
            })
        );
    } else {
        eprintln!(
            "amos: {}/{} edges synced{}",
            applied,
            planned.len(),
            if !failed.is_empty() {
                format!(", {} failed", failed.len())
            } else {
                String::new()
            }
        );
    }

    if !failed.is_empty() && !dry_run {
        bail!("{} relationship sync operation(s) failed", failed.len());
    }
    Ok(())
}
