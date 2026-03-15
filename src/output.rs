use crate::adapter::AdapterRegistry;
use crate::dag::{ComputedStatus, Dag};
use crate::resolver;

/// Format the full DAG state as structured, readable output.
/// Compact lines for done/blocked nodes, expanded with body for ready/in-progress.
/// Bodies of expanded nodes are lazily resolved through the adapter registry.
pub fn format_dag(dag: &Dag, registry: &AdapterRegistry) -> String {
    let mut out = String::new();

    let mut nodes: Vec<_> = dag.all_nodes();
    nodes.sort_by(|a, b| a.name.cmp(&b.name));

    // Validation issues first — cycles, missing deps
    let issues = dag.validate(std::path::Path::new("."));
    if !issues.is_empty() {
        out.push_str("## Issues\n\n");
        for issue in &issues {
            out.push_str(&format!("- {}\n", issue));
        }
        out.push('\n');
    }

    // DAG summary — one line per node
    out.push_str("## DAG\n\n");
    for node in &nodes {
        let status = dag
            .compute_status(&node.name)
            .unwrap_or(ComputedStatus::Blocked);
        let desc = node.description.as_deref().unwrap_or("");

        let display_name = linkify_name(&node.name);
        out.push_str(&format!("- **{}** [{}]", display_name, status));
        if !desc.is_empty() {
            out.push_str(&format!(" — {}", desc));
        }
        out.push('\n');

        if !node.upstream.is_empty() {
            let deps: Vec<String> = node
                .upstream
                .iter()
                .map(|u| {
                    let s = dag
                        .compute_status(u)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    format!("{} [{}]", u, s)
                })
                .collect();
            out.push_str(&format!("  depends on: {}\n", deps.join(", ")));
        }
        if !node.downstream.is_empty() {
            let blocks: Vec<String> = node
                .downstream
                .iter()
                .map(|d| {
                    let s = dag
                        .compute_status(d)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    format!("{} [{}]", d, s)
                })
                .collect();
            out.push_str(&format!("  blocks: {}\n", blocks.join(", ")));
        }
    }

    // Execution order
    if let Some(topo) = dag.topological_sort() {
        let remaining: Vec<&str> = topo
            .iter()
            .filter(|n| dag.compute_status(&n.name) != Some(ComputedStatus::Done))
            .map(|n| n.name.as_str())
            .collect();
        if !remaining.is_empty() {
            out.push_str(&format!(
                "\n## Execution Order\n\n{}\n",
                remaining.join(" → ")
            ));
        }
    }

    // Critical path
    let critical = dag.critical_path();
    if !critical.is_empty() {
        out.push_str(&format!(
            "\n## Critical Path\n\n{}\n",
            critical.join(" → ")
        ));
    }

    // Expanded detail for ready and in-progress nodes — lazy resolution happens here
    let actionable: Vec<_> = nodes
        .iter()
        .filter(|n| {
            matches!(
                dag.compute_status(&n.name),
                Some(ComputedStatus::Ready) | Some(ComputedStatus::InProgress)
            )
        })
        .collect();

    if !actionable.is_empty() {
        out.push_str("\n## Ready / In-Progress\n");

        for node in &actionable {
            let status = dag.compute_status(&node.name).unwrap();
            let display_name = linkify_name(&node.name);
            out.push_str(&format!("\n### {} [{}]\n", display_name, status));

            if let Some(desc) = &node.description {
                out.push_str(&format!("\n{}\n", desc));
            }

            if !node.body.is_empty() {
                // Lazy resolution: resolve @scheme:reference lines through adapters
                let resolved = resolver::resolve_body(&node.body, registry);
                out.push_str(&format!("\n{}\n", resolved));
            }

            if !node.context.is_empty() {
                out.push_str("\n**Context:**\n");
                for ctx in &node.context {
                    out.push_str(&format!("- {}\n", format_context_ref(ctx)));
                }
            }
        }
    }

    out
}

/// Convert `@github:owner/repo#N` names into markdown links.
/// Other names pass through as-is.
fn linkify_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("@github:") {
        if let Some((repo, number)) = rest.split_once('#') {
            return format!(
                "[@github:{}#{}](https://github.com/{}/issues/{})",
                repo, number, repo, number
            );
        }
    }
    name.to_string()
}

fn format_context_ref(ctx: &crate::parser::ContextRef) -> String {
    match ctx {
        crate::parser::ContextRef::LocalFile(path) => path.clone(),
        crate::parser::ContextRef::GitHub {
            owner_repo, path, ..
        } => {
            if let Some(p) = path {
                format!("@github:{}#{}", owner_repo, p)
            } else {
                format!("@github:{}", owner_repo)
            }
        }
        crate::parser::ContextRef::Url(url) => format!("@url:{}", url),
    }
}

/// Format a single node with its fully resolved body.
pub fn format_node(dag: &Dag, name: &str, registry: &AdapterRegistry) -> String {
    let mut out = String::new();

    let Some(node) = dag.get_node(name) else {
        out.push_str(&format!("Node '{}' not found\n", name));
        return out;
    };

    let status = dag
        .compute_status(name)
        .unwrap_or(ComputedStatus::Blocked);
    let display_name = linkify_name(name);

    out.push_str(&format!("## {} [{}]\n", display_name, status));

    if let Some(desc) = &node.description {
        out.push_str(&format!("\n{}\n", desc));
    }

    if !node.upstream.is_empty() {
        let deps: Vec<String> = node
            .upstream
            .iter()
            .map(|u| {
                let s = dag
                    .compute_status(u)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "?".to_string());
                format!("{} [{}]", u, s)
            })
            .collect();
        out.push_str(&format!("\n**depends on:** {}\n", deps.join(", ")));
    }

    if !node.downstream.is_empty() {
        let blocks: Vec<String> = node
            .downstream
            .iter()
            .map(|d| {
                let s = dag
                    .compute_status(d)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "?".to_string());
                format!("{} [{}]", d, s)
            })
            .collect();
        out.push_str(&format!("**blocks:** {}\n", blocks.join(", ")));
    }

    if !node.body.is_empty() {
        let resolved = resolver::resolve_body(&node.body, registry);
        out.push_str(&format!("\n{}\n", resolved));
    }

    if !node.context.is_empty() {
        out.push_str("\n**Context:**\n");
        for ctx in &node.context {
            out.push_str(&format!("- {}\n", format_context_ref(ctx)));
        }
    }

    out.push_str(&format!("\n**Source:** {}\n", node.source_file.display()));

    out
}

/// Format the DAG as an ASCII dependency tree.
pub fn format_graph(dag: &Dag) -> String {
    let mut out = String::new();

    let nodes = dag.all_nodes();
    if nodes.is_empty() {
        return out;
    }

    // Find root nodes (no upstream dependencies, or upstream not in the DAG)
    let mut roots: Vec<&crate::parser::Node> = nodes
        .iter()
        .filter(|n| dag.upstream_of(&n.name).is_empty())
        .copied()
        .collect();
    roots.sort_by(|a, b| a.name.cmp(&b.name));

    // Track what we've printed to avoid duplicates in diamond shapes
    let mut printed = std::collections::HashSet::new();

    for root in &roots {
        format_tree_node(&mut out, dag, &root.name, "", true, &mut printed);
    }

    out
}

fn format_tree_node(
    out: &mut String,
    dag: &Dag,
    name: &str,
    prefix: &str,
    is_last: bool,
    printed: &mut std::collections::HashSet<String>,
) {
    let connector = if prefix.is_empty() {
        ""
    } else if is_last {
        "└── "
    } else {
        "├── "
    };

    let status = dag
        .compute_status(name)
        .unwrap_or(ComputedStatus::Blocked);

    let status_marker = match status {
        ComputedStatus::Done => "✓",
        ComputedStatus::InProgress => "~",
        ComputedStatus::Ready => "●",
        ComputedStatus::Blocked => "○",
    };

    let desc = dag
        .get_node(name)
        .and_then(|n| n.description.as_deref())
        .unwrap_or("");

    let display = shorten_name(name);

    // If already printed (diamond merge), show a back-reference
    if !printed.insert(name.to_string()) {
        out.push_str(&format!(
            "{}{}{} {} (→ see above)\n",
            prefix, connector, status_marker, display
        ));
        return;
    }

    out.push_str(&format!(
        "{}{}{} {} {}\n",
        prefix, connector, status_marker, display, desc
    ));

    let mut children: Vec<_> = dag.downstream_of(name);
    children.sort_by(|a, b| a.name.cmp(&b.name));

    let child_prefix = if prefix.is_empty() {
        "    ".to_string()
    } else if is_last {
        format!("{}    ", prefix)
    } else {
        format!("{}│   ", prefix)
    };

    for (i, child) in children.iter().enumerate() {
        let last = i == children.len() - 1;
        format_tree_node(out, dag, &child.name, &child_prefix, last, printed);
    }
}

/// Shorten @github:owner/repo#N to #N for cleaner tree display.
fn shorten_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("@github:") {
        if let Some((_repo, number)) = rest.split_once('#') {
            return format!("#{}", number);
        }
    }
    name.to_string()
}
