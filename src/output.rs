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

        out.push_str(&format!("- **{}** [{}]", node.name, status));
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
            out.push_str(&format!("\n### {} [{}]\n", node.name, status));

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
