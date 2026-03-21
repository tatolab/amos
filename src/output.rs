use std::collections::HashMap;

use crate::adapter::{AdapterRegistry, ResourceFields};
use crate::dag::Dag;
use crate::resolver;

/// Batch-resolve all adapter-backed nodes in the DAG.
/// Returns a map of node name → resolved facts.
fn resolve_all_facts(dag: &Dag, registry: &AdapterRegistry) -> HashMap<String, ResourceFields> {
    let uri_nodes: Vec<&str> = dag
        .all_nodes()
        .iter()
        .filter(|n| registry.is_resolvable(&n.name))
        .map(|n| n.name.as_str())
        .collect();

    if uri_nodes.is_empty() {
        return HashMap::new();
    }

    registry.resolve_batch(&uri_nodes).unwrap_or_default()
}

/// Format adapter facts as a compact inline string.
/// e.g. "state=CLOSED, labels=bug, priority-high"
fn format_facts(facts: &HashMap<String, String>) -> String {
    if facts.is_empty() {
        return String::new();
    }
    let mut parts: Vec<String> = facts
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();
    parts.sort();
    parts.join(", ")
}

/// Format the full DAG as structured, readable output.
/// Resolves adapter-backed nodes to show live facts.
/// Bodies are lazily resolved through the adapter registry.
pub fn format_dag(dag: &Dag, registry: &AdapterRegistry) -> String {
    let mut out = String::new();

    let mut nodes: Vec<_> = dag.all_nodes();
    nodes.sort_by(|a, b| a.name.cmp(&b.name));

    // Batch-resolve adapter facts for all nodes
    let resolved = resolve_all_facts(dag, registry);

    // Validation issues first — cycles, missing deps
    let issues = dag.validate(std::path::Path::new("."));
    if !issues.is_empty() {
        out.push_str("## Issues\n\n");
        for issue in &issues {
            out.push_str(&format!("- {}\n", issue));
        }
        out.push('\n');
    }

    // DAG summary — one line per node with live adapter facts
    out.push_str("## DAG\n\n");
    for node in &nodes {
        let desc = node.description.as_deref().unwrap_or("");
        let display_name = linkify_name(&node.name);

        // Show adapter facts inline if available
        let facts_tag = resolved
            .get(&node.name)
            .map(|f| format_facts(&f.facts))
            .filter(|s| !s.is_empty())
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();

        out.push_str(&format!("- **{}**{}", display_name, facts_tag));
        if !desc.is_empty() {
            out.push_str(&format!(" — {}", desc));
        }
        out.push('\n');

        if !node.upstream.is_empty() {
            let deps: Vec<String> = node
                .upstream
                .iter()
                .map(|u| format_dep_ref(u, &resolved))
                .collect();
            out.push_str(&format!("  depends on: {}\n", deps.join(", ")));
        }
        if !node.downstream.is_empty() {
            let blocks: Vec<String> = node
                .downstream
                .iter()
                .map(|d| format_dep_ref(d, &resolved))
                .collect();
            out.push_str(&format!("  blocks: {}\n", blocks.join(", ")));
        }
    }

    // Topological order
    if let Some(topo) = dag.topological_sort() {
        let names: Vec<&str> = topo.iter().map(|n| n.name.as_str()).collect();
        if !names.is_empty() {
            out.push_str(&format!(
                "\n## Topological Order\n\n{}\n",
                names.join(" → ")
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

    // Expanded detail for all nodes with bodies — lazy resolution happens here
    let has_body: Vec<_> = nodes.iter().filter(|n| !n.body.is_empty()).collect();

    if !has_body.is_empty() {
        out.push_str("\n## Detail\n");

        for node in &has_body {
            let display_name = linkify_name(&node.name);
            let facts_tag = resolved
                .get(&node.name)
                .map(|f| format_facts(&f.facts))
                .filter(|s| !s.is_empty())
                .map(|s| format!(" [{}]", s))
                .unwrap_or_default();

            out.push_str(&format!("\n### {}{}\n", display_name, facts_tag));

            if let Some(desc) = &node.description {
                out.push_str(&format!("\n{}\n", desc));
            }

            let resolved_body = resolver::resolve_body(&node.body, registry);
            out.push_str(&format!("\n{}\n", resolved_body));

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

/// Format a dependency reference with its adapter facts if resolved.
fn format_dep_ref(name: &str, resolved: &HashMap<String, ResourceFields>) -> String {
    if let Some(fields) = resolved.get(name) {
        let facts = format_facts(&fields.facts);
        if !facts.is_empty() {
            return format!("{} [{}]", name, facts);
        }
    }
    name.to_string()
}

/// Convert `@github:owner/repo#N` names into markdown links.
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

/// Format a single node with its fully resolved body and adapter facts.
pub fn format_node(dag: &Dag, name: &str, registry: &AdapterRegistry) -> String {
    let mut out = String::new();

    let Some(node) = dag.get_node(name) else {
        out.push_str(&format!("Node '{}' not found\n", name));
        return out;
    };

    let display_name = linkify_name(name);

    // Resolve adapter facts for this node
    let adapter_facts = registry
        .resolve(name)
        .and_then(|r| r.ok());

    let facts_tag = adapter_facts
        .as_ref()
        .map(|f| format_facts(&f.facts))
        .filter(|s| !s.is_empty())
        .map(|s| format!(" [{}]", s))
        .unwrap_or_default();

    out.push_str(&format!("## {}{}\n", display_name, facts_tag));

    if let Some(desc) = &node.description {
        out.push_str(&format!("\n{}\n", desc));
    }

    if !node.upstream.is_empty() {
        out.push_str(&format!("\n**depends on:** {}\n", node.upstream.join(", ")));
    }

    if !node.downstream.is_empty() {
        out.push_str(&format!("**blocks:** {}\n", node.downstream.join(", ")));
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

/// Format the DAG as an ASCII dependency tree with live adapter facts.
pub fn format_graph(dag: &Dag, registry: &AdapterRegistry) -> String {
    let mut out = String::new();

    let nodes = dag.all_nodes();
    if nodes.is_empty() {
        return out;
    }

    // Batch-resolve adapter facts
    let resolved = resolve_all_facts(dag, registry);

    // Find root nodes (no upstream dependencies)
    let mut roots: Vec<&crate::parser::Node> = nodes
        .iter()
        .filter(|n| dag.upstream_of(&n.name).is_empty())
        .copied()
        .collect();
    roots.sort_by(|a, b| a.name.cmp(&b.name));

    let mut printed = std::collections::HashSet::new();

    for root in &roots {
        format_tree_node(&mut out, dag, &root.name, "", true, &mut printed, &resolved);
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
    resolved: &HashMap<String, ResourceFields>,
) {
    let connector = if prefix.is_empty() {
        ""
    } else if is_last {
        "└── "
    } else {
        "├── "
    };

    let facts_tag = resolved
        .get(name)
        .map(|f| format_facts(&f.facts))
        .filter(|s| !s.is_empty())
        .map(|s| format!("[{}] ", s))
        .unwrap_or_default();

    let desc = dag
        .get_node(name)
        .and_then(|n| n.description.as_deref())
        .unwrap_or("");

    let display = shorten_name(name);

    if !printed.insert(name.to_string()) {
        out.push_str(&format!(
            "{}{}{}{} (→ see above)\n",
            prefix, connector, facts_tag, display
        ));
        return;
    }

    out.push_str(&format!(
        "{}{}{}{} {}\n",
        prefix, connector, facts_tag, display, desc
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
        format_tree_node(out, dag, &child.name, &child_prefix, last, printed, resolved);
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
