use anyhow::{bail, Result};
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::HashMap;

use crate::parser::Node;

/// The kind of relationship an edge represents.
///
/// The DAG stores multiple concurrent relationship types. Queries filter by
/// kind so blocker analyses can walk Blocks edges independently of associative
/// markers like Related / Duplicates / Supersedes.
///
/// There is intentionally no hierarchical `Parent` edge — grouping is tied to
/// GitHub milestones, not to a parallel notion in amos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Temporal: source blocks target (target waits on source).
    Blocks,
    /// Soft association; stored both directions to keep queries symmetric.
    Related,
    /// Source duplicates target (target is canonical).
    Duplicates,
    /// Source has been replaced by target.
    Supersedes,
}

/// The dependency DAG built from parsed nodes.
///
/// Pure data structure — stores the graph of nodes and typed edges.
/// No status interpretation. Adapters provide facts on demand,
/// the consuming agent reasons about them.
pub struct Dag {
    pub graph: DiGraph<Node, EdgeKind>,
    pub name_to_index: HashMap<String, NodeIndex>,
}

/// A validation issue found during DAG check.
#[derive(Debug)]
pub enum DagIssue {
    DuplicateName {
        name: String,
        files: Vec<String>,
    },
    MissingDependency {
        from_node: String,
        missing_dep: String,
    },
    CycleDetected,
    DanglingContext {
        node: String,
        context_path: String,
    },
}

impl std::fmt::Display for DagIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DagIssue::DuplicateName { name, files } => {
                write!(f, "duplicate name '{}' in: {}", name, files.join(", "))
            }
            DagIssue::MissingDependency {
                from_node,
                missing_dep,
            } => {
                write!(
                    f,
                    "node '{}' depends on '{}' which doesn't exist",
                    from_node, missing_dep
                )
            }
            DagIssue::CycleDetected => write!(f, "cycle detected in dependency graph"),
            DagIssue::DanglingContext { node, context_path } => {
                write!(
                    f,
                    "node '{}' references context '{}' which doesn't exist",
                    node, context_path
                )
            }
        }
    }
}

impl Dag {
    /// Build a DAG from parsed nodes.
    pub fn build(nodes: Vec<Node>) -> Result<Self> {
        let mut graph: DiGraph<Node, EdgeKind> = DiGraph::new();
        let mut name_to_index: HashMap<String, NodeIndex> = HashMap::new();

        // Add all nodes first
        for node in &nodes {
            if name_to_index.contains_key(&node.name) {
                bail!(
                    "duplicate node name '{}' (first seen, then again at {}:{})",
                    node.name,
                    node.source_file.display(),
                    node.line_number
                );
            }
            let idx = graph.add_node(node.clone());
            name_to_index.insert(node.name.clone(), idx);
        }

        // Deduplicate by (source, target, kind) — a node may be reachable by
        // declaration from either side (e.g. parent declares `children: [B]`
        // and B declares `parent: A`), but we only want one edge per relation.
        let mut added_edges: std::collections::HashSet<(NodeIndex, NodeIndex, EdgeKind)> =
            std::collections::HashSet::new();

        let mut add_edge = |graph: &mut DiGraph<Node, EdgeKind>,
                            from: NodeIndex,
                            to: NodeIndex,
                            kind: EdgeKind| {
            if added_edges.insert((from, to, kind)) {
                graph.add_edge(from, to, kind);
            }
        };

        for node in &nodes {
            let node_idx = name_to_index[&node.name];

            // --- Blocks edges: source blocks target ---
            for blocker_name in &node.blocked_by {
                if let Some(&blocker_idx) = name_to_index.get(blocker_name) {
                    add_edge(&mut graph, blocker_idx, node_idx, EdgeKind::Blocks);
                }
            }
            for blocked_name in &node.blocks {
                if let Some(&blocked_idx) = name_to_index.get(blocked_name) {
                    add_edge(&mut graph, node_idx, blocked_idx, EdgeKind::Blocks);
                }
            }

            // --- Related: symmetric, stored both directions ---
            for related_name in &node.related_to {
                if let Some(&related_idx) = name_to_index.get(related_name) {
                    add_edge(&mut graph, node_idx, related_idx, EdgeKind::Related);
                    add_edge(&mut graph, related_idx, node_idx, EdgeKind::Related);
                }
            }

            // --- Duplicates: source duplicates target (target is canonical) ---
            if let Some(dup_name) = &node.duplicates {
                if let Some(&dup_idx) = name_to_index.get(dup_name) {
                    add_edge(&mut graph, node_idx, dup_idx, EdgeKind::Duplicates);
                }
            }

            // --- Supersedes: source has been replaced by target ---
            if let Some(sup_name) = &node.superseded_by {
                if let Some(&sup_idx) = name_to_index.get(sup_name) {
                    add_edge(&mut graph, node_idx, sup_idx, EdgeKind::Supersedes);
                }
            }

        }

        Ok(Dag {
            graph,
            name_to_index,
        })
    }

    /// Get a node by name.
    pub fn get_node(&self, name: &str) -> Option<&Node> {
        self.name_to_index
            .get(name)
            .map(|&idx| &self.graph[idx])
    }

    /// Find shortest path between two nodes (BFS).
    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        let &from_idx = self.name_to_index.get(from)?;
        let &to_idx = self.name_to_index.get(to)?;

        // BFS
        use std::collections::VecDeque;
        let mut queue = VecDeque::new();
        let mut visited: HashMap<NodeIndex, NodeIndex> = HashMap::new();

        queue.push_back(from_idx);
        visited.insert(from_idx, from_idx); // self-parent for start

        while let Some(current) = queue.pop_front() {
            if current == to_idx {
                // Reconstruct path
                let mut path = Vec::new();
                let mut node = to_idx;
                loop {
                    path.push(self.graph[node].name.clone());
                    let parent = visited[&node];
                    if parent == node {
                        break;
                    }
                    node = parent;
                }
                path.reverse();
                return Some(path);
            }

            // Explore outgoing edges (downstream)
            for edge in self.graph.edges_directed(current, Direction::Outgoing) {
                let neighbor = edge.target();
                if let std::collections::hash_map::Entry::Vacant(e) = visited.entry(neighbor) {
                    e.insert(current);
                    queue.push_back(neighbor);
                }
            }
        }

        None
    }

    /// Find the critical path (longest dependency chain) via topological DP.
    pub fn critical_path(&self) -> Vec<String> {
        let topo = match toposort(&self.graph, None) {
            Ok(order) => order,
            Err(_) => return Vec::new(), // cycle
        };

        if topo.is_empty() {
            return Vec::new();
        }

        // DP: for each node, track the longest path ending at that node
        let mut dist: HashMap<NodeIndex, usize> = HashMap::new();
        let mut predecessor: HashMap<NodeIndex, Option<NodeIndex>> = HashMap::new();

        for &idx in &topo {
            dist.insert(idx, 0);
            predecessor.insert(idx, None);
        }

        for &idx in &topo {
            let current_dist = dist[&idx];
            for edge in self.graph.edges_directed(idx, Direction::Outgoing) {
                let target = edge.target();
                if current_dist + 1 > dist[&target] {
                    dist.insert(target, current_dist + 1);
                    predecessor.insert(target, Some(idx));
                }
            }
        }

        // Find the node with maximum distance
        let &end = dist
            .iter()
            .max_by_key(|(_, &d)| d)
            .map(|(idx, _)| idx)
            .unwrap();

        // Reconstruct path
        let mut path = Vec::new();
        let mut current = Some(end);
        while let Some(idx) = current {
            path.push(self.graph[idx].name.clone());
            current = predecessor[&idx];
        }
        path.reverse();
        path
    }

    /// Check for cycles.
    pub fn has_cycle(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// Get topological sort order. Returns None if there's a cycle.
    pub fn topological_sort(&self) -> Option<Vec<&Node>> {
        toposort(&self.graph, None)
            .ok()
            .map(|order| order.into_iter().map(|idx| &self.graph[idx]).collect())
    }

    /// Validate the DAG and return any issues found.
    pub fn validate(&self, scan_root: &std::path::Path) -> Vec<DagIssue> {
        let mut issues = Vec::new();

        // Check for cycles
        if self.has_cycle() {
            issues.push(DagIssue::CycleDetected);
        }

        // Check for missing dependencies across every typed relationship field.
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];

            let mut check = |dep: &String| {
                if !self.name_to_index.contains_key(dep) {
                    issues.push(DagIssue::MissingDependency {
                        from_node: node.name.clone(),
                        missing_dep: dep.clone(),
                    });
                }
            };
            for dep in &node.blocked_by { check(dep); }
            for dep in &node.blocks { check(dep); }
            for dep in &node.related_to { check(dep); }
            if let Some(dep) = &node.duplicates { check(dep); }
            if let Some(dep) = &node.superseded_by { check(dep); }

            // Check local file context references
            for ctx in &node.context {
                if let crate::parser::ContextRef::LocalFile(path) = ctx {
                    let full_path = scan_root.join(path);
                    if !full_path.exists() {
                        issues.push(DagIssue::DanglingContext {
                            node: node.name.clone(),
                            context_path: path.clone(),
                        });
                    }
                }
            }
        }

        issues
    }

    /// Get all node names in the DAG.
    pub fn all_nodes(&self) -> Vec<&Node> {
        self.graph
            .node_indices()
            .map(|idx| &self.graph[idx])
            .collect()
    }

    /// Get upstream neighbors across all edge kinds.
    pub fn upstream_of(&self, name: &str) -> Vec<&Node> {
        let Some(&idx) = self.name_to_index.get(name) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|edge| &self.graph[edge.source()])
            .collect()
    }

    /// Get downstream neighbors across all edge kinds.
    pub fn downstream_of(&self, name: &str) -> Vec<&Node> {
        let Some(&idx) = self.name_to_index.get(name) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|edge| &self.graph[edge.target()])
            .collect()
    }

    /// Generic neighbor lookup: neighbors reachable via edges of `kind` in
    /// `direction`. All kind-specific helpers below are thin wrappers.
    fn neighbors_by_kind(
        &self,
        name: &str,
        kind: EdgeKind,
        direction: Direction,
    ) -> Vec<&Node> {
        let Some(&idx) = self.name_to_index.get(name) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, direction)
            .filter(|edge| *edge.weight() == kind)
            .map(|edge| {
                let neighbor = match direction {
                    Direction::Outgoing => edge.target(),
                    Direction::Incoming => edge.source(),
                };
                &self.graph[neighbor]
            })
            .collect()
    }

    /// Nodes this node blocks (outgoing Blocks edges).
    pub fn blocks_of(&self, name: &str) -> Vec<&Node> {
        self.neighbors_by_kind(name, EdgeKind::Blocks, Direction::Outgoing)
    }

    /// Nodes blocking this node (incoming Blocks edges).
    pub fn blocked_by_of(&self, name: &str) -> Vec<&Node> {
        self.neighbors_by_kind(name, EdgeKind::Blocks, Direction::Incoming)
    }

    /// Nodes softly related to this one (outgoing Related edges; symmetric).
    pub fn related_of(&self, name: &str) -> Vec<&Node> {
        self.neighbors_by_kind(name, EdgeKind::Related, Direction::Outgoing)
    }

    /// Canonical node this one duplicates (outgoing Duplicates edge).
    pub fn duplicates_of(&self, name: &str) -> Option<&Node> {
        self.neighbors_by_kind(name, EdgeKind::Duplicates, Direction::Outgoing)
            .into_iter()
            .next()
    }

    /// Node that supersedes this one (outgoing Supersedes edge).
    pub fn superseded_by_of(&self, name: &str) -> Option<&Node> {
        self.neighbors_by_kind(name, EdgeKind::Supersedes, Direction::Outgoing)
            .into_iter()
            .next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Node;
    use std::path::PathBuf;

    fn make_node(name: &str, blocked_by: Vec<&str>, blocks: Vec<&str>) -> Node {
        Node {
            name: name.to_string(),
            description: None,
            blocked_by: blocked_by.into_iter().map(String::from).collect(),
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
    fn test_graph_structure() {
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec!["a"], vec!["c"]),
            make_node("c", vec!["b"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();

        assert!(dag.upstream_of("a").is_empty());
        assert_eq!(dag.upstream_of("b").len(), 1);
        assert_eq!(dag.upstream_of("b")[0].name, "a");
        assert_eq!(dag.downstream_of("a").len(), 1);
        assert_eq!(dag.downstream_of("a")[0].name, "b");
    }

    #[test]
    fn test_shortest_path() {
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec!["a"], vec!["c"]),
            make_node("c", vec!["b"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let path = dag.shortest_path("a", "c");
        assert_eq!(path, Some(vec!["a".to_string(), "b".to_string(), "c".to_string()]));
    }

    #[test]
    fn test_critical_path() {
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec!["a"], vec!["c"]),
            make_node("c", vec!["b"], vec![]),
            make_node("d", vec![], vec![]), // isolated, shorter
        ];
        let dag = Dag::build(nodes).unwrap();
        let cp = dag.critical_path();
        assert_eq!(cp, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_duplicate_name_fails() {
        let nodes = vec![
            make_node("a", vec![], vec![]),
            make_node("a", vec![], vec![]),
        ];
        assert!(Dag::build(nodes).is_err());
    }

    #[test]
    fn test_missing_dep_validation() {
        let nodes = vec![
            make_node("a", vec!["nonexistent"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let issues = dag.validate(std::path::Path::new("/tmp"));
        assert!(issues.iter().any(|i| matches!(i, DagIssue::MissingDependency { .. })));
    }

    #[test]
    fn test_blocks_edges() {
        let mut a = make_node("a", vec![], vec![]);
        a.blocks = vec!["b".to_string()];
        let nodes = vec![a, make_node("b", vec![], vec![])];
        let dag = Dag::build(nodes).unwrap();

        assert_eq!(dag.blocks_of("a").len(), 1);
        assert_eq!(dag.blocked_by_of("b").len(), 1);
    }

    #[test]
    fn test_related_is_symmetric() {
        let mut a = make_node("a", vec![], vec![]);
        a.related_to = vec!["b".to_string()];
        let nodes = vec![a, make_node("b", vec![], vec![])];
        let dag = Dag::build(nodes).unwrap();

        assert_eq!(dag.related_of("a").len(), 1);
        assert_eq!(dag.related_of("b").len(), 1); // stored both directions
    }

    #[test]
    fn test_duplicates_and_supersedes() {
        let mut a = make_node("a", vec![], vec![]);
        a.duplicates = Some("canonical".to_string());
        let mut b = make_node("b", vec![], vec![]);
        b.superseded_by = Some("new-version".to_string());
        let nodes = vec![
            a,
            b,
            make_node("canonical", vec![], vec![]),
            make_node("new-version", vec![], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();

        assert_eq!(dag.duplicates_of("a").map(|n| n.name.as_str()), Some("canonical"));
        assert_eq!(dag.superseded_by_of("b").map(|n| n.name.as_str()), Some("new-version"));
    }

    #[test]
    fn test_legacy_down_creates_blocks_edge() {
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec![], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();

        assert_eq!(dag.blocks_of("a").len(), 1);
        assert_eq!(dag.blocked_by_of("b").len(), 1);
    }
}
