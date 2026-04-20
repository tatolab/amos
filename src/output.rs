use std::collections::{HashMap, HashSet};

use crate::adapter::{AdapterRegistry, ResourceFields};
use crate::dag::Dag;
use crate::resolver;

/// Compare two node names with natural-numeric ordering.
///
/// Handles `@github:owner/repo#N` by extracting the trailing `#N` and comparing
/// numerically when both sides share the same prefix, so `#99` sorts before
/// `#100` instead of after it. Falls back to lexicographic comparison for
/// non-numeric names or mixed shapes.
pub fn numeric_aware_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    if let (Some((a_prefix, a_num)), Some((b_prefix, b_num))) = (
        split_numeric_suffix(a),
        split_numeric_suffix(b),
    ) {
        if a_prefix == b_prefix {
            return a_num.cmp(&b_num);
        }
    }
    a.cmp(b)
}

/// Split a name like `@github:owner/repo#123` into (`@github:owner/repo#`, 123).
/// Returns None if the name doesn't end in `#<digits>`.
fn split_numeric_suffix(s: &str) -> Option<(&str, u64)> {
    let hash_pos = s.rfind('#')?;
    let (prefix, rest) = s.split_at(hash_pos + 1);
    let n: u64 = rest.parse().ok()?;
    Some((prefix, n))
}

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

        if !node.blocked_by.is_empty() {
            let deps: Vec<String> = node
                .blocked_by
                .iter()
                .map(|u| format_dep_ref(u, &resolved))
                .collect();
            out.push_str(&format!("  blocked_by: {}\n", deps.join(", ")));
        }
        if !node.blocks.is_empty() {
            let blocks: Vec<String> = node
                .blocks
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

    if !node.blocked_by.is_empty() {
        out.push_str(&format!("\n**blocked_by:** {}\n", node.blocked_by.join(", ")));
    }

    if !node.blocks.is_empty() {
        out.push_str(&format!("**blocks:** {}\n", node.blocks.join(", ")));
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
///
/// Prepends a `## Issues` section when validation problems are present (missing
/// deps, cycles, dangling context). The scan_root is needed to resolve local
/// file context references when validating.
pub fn format_graph(
    dag: &Dag,
    registry: &AdapterRegistry,
    scan_root: &std::path::Path,
) -> String {
    let mut out = String::new();

    let nodes = dag.all_nodes();
    if nodes.is_empty() {
        return out;
    }

    // Surface any validation issues before the tree so users notice them.
    let issues = dag.validate(scan_root);
    if !issues.is_empty() {
        out.push_str("## Issues\n\n");
        for issue in &issues {
            out.push_str(&format!("- {}\n", issue));
        }
        out.push('\n');
    }

    // Batch-resolve adapter facts
    let resolved = resolve_all_facts(dag, registry);

    // Roots = nodes with no incoming Blocks edges (nothing blocks them).
    // The tree walks Blocks-kind edges only, so Related/Duplicates/Supersedes
    // don't create extra branches or loops.
    let mut roots: Vec<&crate::parser::Node> = nodes
        .iter()
        .filter(|n| dag.blocked_by_of(&n.name).is_empty())
        .copied()
        .collect();
    roots.sort_by(|a, b| numeric_aware_cmp(&a.name, &b.name));

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

    // Children are ordered by the parent's declared `blocks:` list (preserving
    // authoring order), with any additional DAG-derived children (e.g. edges
    // created by another node's `blocked_by:` pointing at this node) appended
    // at the end sorted numerically. Only Blocks-kind edges count — Related,
    // Duplicates, and Supersedes don't create tree branches.
    let dag_children = dag.blocks_of(name);
    let dag_child_names: HashSet<&str> =
        dag_children.iter().map(|n| n.name.as_str()).collect();

    let mut children: Vec<&crate::parser::Node> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    if let Some(parent_node) = dag.get_node(name) {
        for declared in &parent_node.blocks {
            if dag_child_names.contains(declared.as_str()) {
                if let Some(child) = dag.get_node(declared) {
                    if seen.insert(declared.clone()) {
                        children.push(child);
                    }
                }
            }
        }
    }

    let mut extras: Vec<&crate::parser::Node> = dag_children
        .iter()
        .filter(|n| !seen.contains(&n.name))
        .copied()
        .collect();
    extras.sort_by(|a, b| numeric_aware_cmp(&a.name, &b.name));
    children.extend(extras);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::AdapterRegistry;
    use crate::dag::Dag;
    use crate::parser::Node;
    use std::cmp::Ordering;
    use std::path::PathBuf;

    fn node(name: &str, blocks: Vec<&str>) -> Node {
        Node {
            name: name.to_string(),
            description: None,
            blocked_by: Vec::new(),
            blocks: blocks.into_iter().map(String::from).collect(),
            related_to: Vec::new(),
            duplicates: None,
            superseded_by: None,
            labels: Vec::new(),
            priority: None,
            context: Vec::new(),
            adapters: std::collections::HashMap::new(),
            source_file: PathBuf::from("test.md"),
            line_number: 1,
            body: String::new(),
        }
    }

    #[test]
    fn numeric_cmp_sorts_github_issues_numerically() {
        let a = "@github:tatolab/streamlib#99";
        let b = "@github:tatolab/streamlib#100";
        assert_eq!(numeric_aware_cmp(a, b), Ordering::Less);
    }

    #[test]
    fn numeric_cmp_equal_same_number() {
        let a = "@github:tatolab/streamlib#42";
        let b = "@github:tatolab/streamlib#42";
        assert_eq!(numeric_aware_cmp(a, b), Ordering::Equal);
    }

    #[test]
    fn numeric_cmp_different_prefixes_lexicographic() {
        let a = "@github:tatolab/streamlib#1";
        let b = "@github:tatolab/amos#100";
        // Different prefixes -> lexicographic (streamlib > amos, regardless of number)
        assert_eq!(numeric_aware_cmp(a, b), Ordering::Greater);
    }

    #[test]
    fn numeric_cmp_non_numeric_names_lexicographic() {
        assert_eq!(numeric_aware_cmp("alpha", "beta"), Ordering::Less);
    }

    #[test]
    fn tree_preserves_declared_downstream_order() {
        // Parent declares children in non-alphabetic order: c, a, b.
        let nodes = vec![
            node("p", vec!["c", "a", "b"]),
            node("a", vec![]),
            node("b", vec![]),
            node("c", vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let registry = AdapterRegistry::new();
        let tree = format_graph(&dag, &registry, std::path::Path::new("."));

        // Children should appear in declared order: c, a, b
        let c_pos = tree.find("── c").expect("c present");
        let a_pos = tree.find("── a").expect("a present");
        let b_pos = tree.find("── b").expect("b present");
        assert!(c_pos < a_pos, "c should come before a (declared order)");
        assert!(a_pos < b_pos, "a should come before b (declared order)");
    }

    #[test]
    fn graph_prepends_issues_when_validation_fails() {
        // A node references a nonexistent blocks target — validate() should
        // emit a MissingDependency issue, and format_graph should surface it.
        let n = node("a", vec!["ghost"]);
        let dag = Dag::build(vec![n]).unwrap();
        let registry = AdapterRegistry::new();
        let tree = format_graph(&dag, &registry, std::path::Path::new("."));

        assert!(
            tree.contains("## Issues"),
            "expected Issues section, got: {}",
            tree
        );
        assert!(
            tree.contains("ghost"),
            "expected missing dep name in issues section, got: {}",
            tree
        );
    }

    #[test]
    fn graph_omits_issues_section_when_clean() {
        let nodes = vec![node("a", vec!["b"]), node("b", vec![])];
        let dag = Dag::build(nodes).unwrap();
        let registry = AdapterRegistry::new();
        let tree = format_graph(&dag, &registry, std::path::Path::new("."));
        assert!(
            !tree.contains("## Issues"),
            "expected no Issues section when DAG is clean, got: {}",
            tree
        );
    }

    #[test]
    fn tree_appends_undeclared_children_numerically_sorted() {
        // Parent declares only #320 and #322; #321 becomes a child via its
        // own `blocked_by:` ref, so it's not in the parent's declared list.
        let p_name = "@github:org/repo#1";
        let c320 = "@github:org/repo#320";
        let c321 = "@github:org/repo#321";
        let c322 = "@github:org/repo#322";

        let parent = node(p_name, vec![c320, c322]);
        let mut n321 = node(c321, vec![]);
        n321.blocked_by = vec![p_name.to_string()]; // adds edge p -> 321 via blocked_by:

        let nodes = vec![
            parent,
            node(c320, vec![]),
            n321,
            node(c322, vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let registry = AdapterRegistry::new();
        let tree = format_graph(&dag, &registry, std::path::Path::new("."));

        let p320 = tree.find("#320").expect("320 present");
        let p321 = tree.find("#321").expect("321 present");
        let p322 = tree.find("#322").expect("322 present");
        // Declared order first (320, 322), then extras (321)
        assert!(p320 < p322, "declared 320 before declared 322");
        assert!(p322 < p321, "undeclared 321 appears after declared list");
    }
}
