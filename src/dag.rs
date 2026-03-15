use anyhow::{bail, Result};
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::HashMap;

use crate::parser::Node;
use crate::status::ManualStatus;

/// Computed status of a node in the DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputedStatus {
    Done,
    InProgress,
    Ready,
    Blocked,
}

impl std::fmt::Display for ComputedStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComputedStatus::Done => write!(f, "done"),
            ComputedStatus::InProgress => write!(f, "in-progress"),
            ComputedStatus::Ready => write!(f, "ready"),
            ComputedStatus::Blocked => write!(f, "blocked"),
        }
    }
}

/// The dependency DAG built from parsed nodes.
pub struct Dag {
    pub graph: DiGraph<Node, ()>,
    pub name_to_index: HashMap<String, NodeIndex>,
    status_overlay: HashMap<String, ManualStatus>,
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
    /// Apply external status overlay (from .amos-status file).
    pub fn apply_status_overlay(&mut self, statuses: HashMap<String, ManualStatus>) {
        self.status_overlay = statuses;
    }

    /// Build a DAG from parsed nodes.
    pub fn build(nodes: Vec<Node>) -> Result<Self> {
        let mut graph = DiGraph::new();
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

        // Add edges (deduplicated — both sides may declare the same relationship)
        // Edge direction: upstream -> downstream (upstream must complete first)
        // So if B has `up:A`, edge is A -> B
        // If A has `down:B`, edge is also A -> B
        let mut added_edges: std::collections::HashSet<(NodeIndex, NodeIndex)> =
            std::collections::HashSet::new();

        for node in &nodes {
            let node_idx = name_to_index[&node.name];

            for upstream_name in &node.upstream {
                if let Some(&upstream_idx) = name_to_index.get(upstream_name) {
                    let edge = (upstream_idx, node_idx);
                    if added_edges.insert(edge) {
                        graph.add_edge(upstream_idx, node_idx, ());
                    }
                }
                // Missing deps are handled in validate(), not here
            }

            for downstream_name in &node.downstream {
                if let Some(&downstream_idx) = name_to_index.get(downstream_name) {
                    let edge = (node_idx, downstream_idx);
                    if added_edges.insert(edge) {
                        graph.add_edge(node_idx, downstream_idx, ());
                    }
                }
            }
        }

        Ok(Dag {
            graph,
            name_to_index,
            status_overlay: HashMap::new(),
        })
    }

    /// Get a node by name.
    pub fn get_node(&self, name: &str) -> Option<&Node> {
        self.name_to_index
            .get(name)
            .map(|&idx| &self.graph[idx])
    }

    /// Compute the status of a single node.
    pub fn compute_status(&self, name: &str) -> Option<ComputedStatus> {
        let &idx = self.name_to_index.get(name)?;
        let mut visited = std::collections::HashSet::new();
        Some(self.compute_status_for_index(idx, &mut visited))
    }

    fn compute_status_for_index(
        &self,
        idx: NodeIndex,
        visited: &mut std::collections::HashSet<NodeIndex>,
    ) -> ComputedStatus {
        if !visited.insert(idx) {
            // Cycle — can never resolve to done
            return ComputedStatus::Blocked;
        }

        let node = &self.graph[idx];

        // Check status overlay
        if let Some(&status) = self.status_overlay.get(&node.name) {
            match status {
                ManualStatus::Done => return ComputedStatus::Done,
                ManualStatus::InProgress => return ComputedStatus::InProgress,
            }
        }

        // Rule 3 & 4: check upstream deps
        let all_upstream_done = self
            .graph
            .edges_directed(idx, Direction::Incoming)
            .all(|edge| {
                let upstream_idx = edge.source();
                self.compute_status_for_index(upstream_idx, visited) == ComputedStatus::Done
            });

        if all_upstream_done {
            ComputedStatus::Ready
        } else {
            ComputedStatus::Blocked
        }
    }

    /// Find all nodes with status Ready.
    pub fn find_ready(&self) -> Vec<&Node> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let mut visited = std::collections::HashSet::new();
                self.compute_status_for_index(idx, &mut visited) == ComputedStatus::Ready
            })
            .map(|idx| &self.graph[idx])
            .collect()
    }

    /// Find all blocked nodes with the names of what blocks them.
    pub fn find_blocked_with_blockers(&self) -> Vec<(&Node, Vec<String>)> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let mut visited = std::collections::HashSet::new();
                self.compute_status_for_index(idx, &mut visited) == ComputedStatus::Blocked
            })
            .map(|idx| {
                let blockers: Vec<String> = self
                    .graph
                    .edges_directed(idx, Direction::Incoming)
                    .filter(|edge| {
                        let mut visited = std::collections::HashSet::new();
                        self.compute_status_for_index(edge.source(), &mut visited)
                            != ComputedStatus::Done
                    })
                    .map(|edge| self.graph[edge.source()].name.clone())
                    .collect();
                (&self.graph[idx], blockers)
            })
            .collect()
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

        // Check for missing dependencies
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];

            for dep in &node.upstream {
                if !self.name_to_index.contains_key(dep) {
                    issues.push(DagIssue::MissingDependency {
                        from_node: node.name.clone(),
                        missing_dep: dep.clone(),
                    });
                }
            }

            for dep in &node.downstream {
                if !self.name_to_index.contains_key(dep) {
                    issues.push(DagIssue::MissingDependency {
                        from_node: node.name.clone(),
                        missing_dep: dep.clone(),
                    });
                }
            }

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

    /// Get upstream neighbors of a node.
    pub fn upstream_of(&self, name: &str) -> Vec<&Node> {
        let Some(&idx) = self.name_to_index.get(name) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|edge| &self.graph[edge.source()])
            .collect()
    }

    /// Get downstream neighbors of a node.
    pub fn downstream_of(&self, name: &str) -> Vec<&Node> {
        let Some(&idx) = self.name_to_index.get(name) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .map(|edge| &self.graph[edge.target()])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Node;
    use std::path::PathBuf;

    fn make_node(name: &str, upstream: Vec<&str>, downstream: Vec<&str>) -> Node {
        Node {
            name: name.to_string(),
            description: None,
            upstream: upstream.into_iter().map(String::from).collect(),
            downstream: downstream.into_iter().map(String::from).collect(),
            context: Vec::new(),
            source_file: PathBuf::from("test.md"),
            line_number: 1,
            body: String::new(),
        }
    }

    fn with_status(mut dag: Dag, statuses: Vec<(&str, ManualStatus)>) -> Dag {
        let map: HashMap<String, ManualStatus> = statuses
            .into_iter()
            .map(|(n, s)| (n.to_string(), s))
            .collect();
        dag.apply_status_overlay(map);
        dag
    }

    #[test]
    fn test_simple_chain_status() {
        // A -> B -> C, A is done
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec!["a"], vec!["c"]),
            make_node("c", vec!["b"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let dag = with_status(dag, vec![("a", ManualStatus::Done)]);

        assert_eq!(dag.compute_status("a"), Some(ComputedStatus::Done));
        assert_eq!(dag.compute_status("b"), Some(ComputedStatus::Ready));
        assert_eq!(dag.compute_status("c"), Some(ComputedStatus::Blocked));
    }

    #[test]
    fn test_no_deps_is_ready() {
        let nodes = vec![make_node("a", vec![], vec![])];
        let dag = Dag::build(nodes).unwrap();
        assert_eq!(dag.compute_status("a"), Some(ComputedStatus::Ready));
    }

    #[test]
    fn test_in_progress_status() {
        let nodes = vec![make_node("a", vec![], vec![])];
        let dag = Dag::build(nodes).unwrap();
        let dag = with_status(dag, vec![("a", ManualStatus::InProgress)]);
        assert_eq!(dag.compute_status("a"), Some(ComputedStatus::InProgress));
    }

    #[test]
    fn test_find_ready() {
        let nodes = vec![
            make_node("a", vec![], vec!["b"]),
            make_node("b", vec!["a"], vec!["c"]),
            make_node("c", vec!["b"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        let dag = with_status(dag, vec![("a", ManualStatus::Done)]);
        let ready: Vec<&str> = dag.find_ready().iter().map(|n| n.name.as_str()).collect();
        assert_eq!(ready, vec!["b"]);
    }

    #[test]
    fn test_blocked_with_blockers() {
        let nodes = vec![
            make_node("a", vec![], vec!["c"]),
            make_node("b", vec![], vec!["c"]),
            make_node("c", vec!["a", "b"], vec![]),
        ];
        let dag = Dag::build(nodes).unwrap();
        // a and b are ready (no upstream), c is blocked by a and b
        // But a and b are "ready" not "done", so c is blocked
        let blocked = dag.find_blocked_with_blockers();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].0.name, "c");
        let mut blockers = blocked[0].1.clone();
        blockers.sort();
        assert_eq!(blockers, vec!["a", "b"]);
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
}
