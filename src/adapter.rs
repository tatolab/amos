use anyhow::{bail, Result};
use std::collections::HashMap;

/// Resolved fields from an adapter. Each field is optional —
/// the adapter returns whatever it can resolve for the given URI.
#[derive(Debug, Default, Clone)]
pub struct ResourceFields {
    pub name: Option<String>,
    pub description: Option<String>,
    pub body: Option<String>,
    /// Raw facts from the external system. The adapter decides what to include.
    /// Amos passes these through without interpretation — the consuming agent
    /// reads the facts and reasons about them.
    /// Examples: {"state": "CLOSED", "labels": "bug, priority-high"}
    pub facts: HashMap<String, String>,
}

/// A "node view" materialized directly from an adapter's backing system — e.g.
/// a GitHub issue enumerated from a milestone. Lets amos see issues that don't
/// have a local plan file.
///
/// Adapters emit these; `main.rs` merges them with local parsed nodes to build
/// a complete DAG. Local nodes always win for AI-specific fields (body,
/// context, labels_local); the adapter wins for state + native relationship
/// edges.
#[derive(Debug, Clone, Default)]
pub struct AdapterNode {
    /// Canonical amos name (e.g. `@github:tatolab/streamlib#388`).
    pub name: String,
    pub title: String,
    /// Raw facts — state, milestone, labels (comma-joined). Same shape as
    /// `ResourceFields::facts`.
    pub facts: HashMap<String, String>,
    /// Canonical names of nodes blocking this one (upstream → this is blocked
    /// by these).
    pub blocked_by: Vec<String>,
    /// Canonical names of nodes this one blocks (downstream).
    pub blocks: Vec<String>,
    /// Parent issue (GitHub sub-issue hierarchy).
    pub parent: Option<String>,
    /// Child issues (GitHub sub-issues).
    pub sub_issues: Vec<String>,
}

/// Summary of a milestone pulled directly from the adapter. Counts are
/// authoritative — they reflect every issue in the milestone, not just ones
/// with a local plan file.
#[derive(Debug, Clone)]
pub struct MilestoneInfo {
    pub title: String,
    pub state: String,
    pub open_count: usize,
    pub closed_count: usize,
}

/// Kinds of native relationships amos knows how to create. Adapters may
/// support a subset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipKind {
    /// `from` is blocked by `to` (GitHub: addBlockedBy).
    BlockedBy,
    /// `from` blocks `to` (inverse of BlockedBy — adapter may implement via
    /// the reverse mutation).
    Blocks,
    /// `from` is a sub-issue of `to` (GitHub: addSubIssue with parent = `to`).
    SubIssueOf,
}

/// Declarative issue spec passed to `create_issue`. Lets the `amos-file`
/// skill produce a draft, show it to the user, and hand off a single
/// structured payload for atomic creation.
#[derive(Debug, Clone, Default)]
pub struct IssueSpec {
    pub title: String,
    pub body: String,
    pub milestone: Option<String>,
    pub labels: Vec<String>,
    /// GitHub issue type — e.g. "Bug", "Feature", "Task". Case-insensitive
    /// match against the repo's configured issue types. `None` leaves the
    /// type unset.
    pub issue_type: Option<String>,
    /// References of nodes this one is blocked by (canonical names).
    pub blocked_by: Vec<String>,
    /// References this one blocks.
    pub blocks: Vec<String>,
    /// If set, the new issue is a sub-issue of this one.
    pub sub_issue_of: Option<String>,
}

/// Result of a successful issue creation.
#[derive(Debug, Clone)]
pub struct CreatedIssue {
    /// Canonical amos name (e.g. `@github:tatolab/streamlib#395`).
    pub name: String,
    /// Tracker-specific issue number.
    pub number: u64,
    /// URL users can click to view the issue.
    pub url: String,
}

/// Adapter trait — resolves URIs within a registered scheme.
///
/// An adapter handles a URI scheme (e.g. `gh:`, `jira:`, `linear:`).
/// When amos encounters a URI like `gh:tatolab/amos#15`, it splits
/// on the first `:`, finds the adapter registered for `gh`, and
/// calls `resolve("tatolab/amos#15")`.
///
/// The adapter returns whatever fields it can resolve. These overlay
/// the node's local frontmatter values.
pub trait Adapter: Send + Sync {
    /// The URI scheme this adapter handles (e.g. "gh", "jira").
    fn scheme(&self) -> &str;

    /// Resolve a URI reference (everything after `scheme:`).
    fn resolve(&self, reference: &str) -> Result<ResourceFields>;

    /// Batch resolve multiple references. Default calls resolve() in a loop.
    /// Adapters can override for efficiency (e.g. one API call for many issues).
    fn resolve_batch(&self, references: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        let mut results = HashMap::new();
        for reference in references {
            results.insert(reference.to_string(), self.resolve(reference)?);
        }
        Ok(results)
    }

    /// Send a message to the external system for this node.
    /// The adapter decides how to record it — comment, log, status update, etc.
    /// Default is a no-op — adapters that support write-back override this.
    fn notify(&self, _reference: &str, _message: &str) -> Result<()> {
        Ok(())
    }

    /// List every milestone this adapter knows about. Lets `amos milestones`
    /// report accurate counts regardless of which issues have local plan
    /// files. Default is empty — adapters that don't model milestones return
    /// nothing.
    fn list_milestones(&self) -> Result<Vec<MilestoneInfo>> {
        Ok(Vec::new())
    }

    /// Enumerate all nodes in a milestone, including native relationship
    /// edges (GitHub's typed `blockedBy` / `blocking` / `parent` / `subIssues`
    /// fields). Default is empty.
    fn list_nodes_in_milestone(&self, _milestone: &str) -> Result<Vec<AdapterNode>> {
        Ok(Vec::new())
    }

    /// Create a native relationship between two references. `from` and `to`
    /// are adapter-local references (not full URIs — the scheme prefix is
    /// already stripped). Default errors — adapters that support write-back
    /// override.
    fn add_relationship(
        &self,
        _from: &str,
        _to: &str,
        _kind: RelationshipKind,
    ) -> Result<()> {
        bail!("adapter does not support creating relationships")
    }

    /// Create a new issue/task in the backing system. Returns the canonical
    /// amos name + tracker-specific number + clickable URL. Milestone,
    /// labels, and relationships in the spec are applied atomically — if
    /// any relationship fails, creation is still reported as successful but
    /// the error is attached to the partial-failure list returned by the
    /// caller.
    fn create_issue(&self, _spec: &IssueSpec) -> Result<CreatedIssue> {
        bail!("adapter does not support creating issues")
    }
}

/// Registry of adapters keyed by URI scheme.
pub struct AdapterRegistry {
    adapters: HashMap<String, Box<dyn Adapter>>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        AdapterRegistry {
            adapters: HashMap::new(),
        }
    }

    /// Register an adapter for its scheme.
    pub fn register(&mut self, adapter: Box<dyn Adapter>) {
        let scheme = adapter.scheme().to_string();
        self.adapters.insert(scheme, adapter);
    }

    /// Send a message to the adapter that owns a URI.
    /// Best-effort — logs failures but doesn't propagate errors.
    pub fn notify(&self, uri: &str, message: &str) {
        let Some((scheme, reference)) = self.parse_uri(uri) else {
            return;
        };
        let Some(adapter) = self.adapters.get(scheme) else {
            return;
        };
        if let Err(e) = adapter.notify(reference, message) {
            eprintln!("amos: notify failed for {}: {}", uri, e);
        }
    }

    /// Check if a string looks like a URI with a registered scheme.
    pub fn is_resolvable(&self, value: &str) -> bool {
        self.parse_uri(value)
            .is_some_and(|(scheme, _)| self.adapters.contains_key(scheme))
    }

    /// Resolve a URI through the matching adapter.
    /// Returns None if the URI scheme isn't registered.
    pub fn resolve(&self, uri: &str) -> Option<Result<ResourceFields>> {
        let (scheme, reference) = self.parse_uri(uri)?;
        let adapter = self.adapters.get(scheme)?;
        Some(adapter.resolve(reference))
    }

    /// Collect all resolvable URIs from a list, batch-resolve per adapter.
    pub fn resolve_batch(&self, uris: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        // Group URIs by scheme, remembering the original URI string for re-keying
        let mut by_scheme: HashMap<&str, Vec<(&str, &str)>> = HashMap::new();
        for uri in uris {
            if let Some((scheme, reference)) = self.parse_uri(uri) {
                if self.adapters.contains_key(scheme) {
                    by_scheme.entry(scheme).or_default().push((uri, reference));
                }
            }
        }

        let mut all_results = HashMap::new();
        for (scheme, entries) in by_scheme {
            let references: Vec<&str> = entries.iter().map(|(_, r)| *r).collect();
            let adapter = &self.adapters[scheme];
            let results = adapter.resolve_batch(&references)?;
            // Re-key with original URI (preserves @prefix)
            for (original_uri, reference) in &entries {
                if let Some(fields) = results.get(*reference) {
                    all_results.insert(original_uri.to_string(), fields.clone());
                }
            }
        }
        Ok(all_results)
    }

    /// List every milestone across every registered adapter. Useful for
    /// `amos milestones` — returns accurate counts from the source of truth,
    /// not from whatever subset of issues has local plan files.
    pub fn list_all_milestones(&self) -> Vec<MilestoneInfo> {
        let mut out: Vec<MilestoneInfo> = Vec::new();
        for adapter in self.adapters.values() {
            match adapter.list_milestones() {
                Ok(ms) => out.extend(ms),
                Err(e) => eprintln!("amos: {} adapter milestone listing failed: {}", adapter.scheme(), e),
            }
        }
        out
    }

    /// Enumerate all nodes in `milestone` from every adapter.
    pub fn list_nodes_in_milestone(&self, milestone: &str) -> Vec<AdapterNode> {
        let mut out: Vec<AdapterNode> = Vec::new();
        for adapter in self.adapters.values() {
            match adapter.list_nodes_in_milestone(milestone) {
                Ok(ns) => out.extend(ns),
                Err(e) => eprintln!("amos: {} adapter milestone enumeration failed: {}", adapter.scheme(), e),
            }
        }
        out
    }

    /// Create a new issue via the named adapter scheme (e.g. "github"). The
    /// caller decides which scheme to use — there's no inference from the
    /// issue body, since a brand-new issue has no canonical name yet.
    pub fn create_issue(&self, scheme: &str, spec: &IssueSpec) -> Result<CreatedIssue> {
        let Some(adapter) = self.adapters.get(scheme) else {
            bail!("no adapter registered for scheme '{}'", scheme);
        };
        adapter.create_issue(spec)
    }

    /// Add a native relationship via the adapter that owns `from`.
    pub fn add_relationship(
        &self,
        from: &str,
        to: &str,
        kind: RelationshipKind,
    ) -> Result<()> {
        let Some((scheme, from_ref)) = self.parse_uri(from) else {
            bail!("'{}' is not a recognized adapter URI", from);
        };
        let Some(adapter) = self.adapters.get(scheme) else {
            bail!("no adapter registered for scheme '{}'", scheme);
        };
        let to_ref = self
            .parse_uri(to)
            .and_then(|(target_scheme, target_ref)| {
                (target_scheme == scheme).then_some(target_ref)
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "relationship targets must share a scheme: {} vs {}",
                    from,
                    to
                )
            })?;
        adapter.add_relationship(from_ref, to_ref, kind)
    }

    /// Parse a string into (scheme, reference) if it looks like a URI.
    /// Handles both `scheme:reference` and `@scheme:reference` formats.
    fn parse_uri<'a>(&self, value: &'a str) -> Option<(&'a str, &'a str)> {
        // Strip leading @ if present
        let value = value.strip_prefix('@').unwrap_or(value);
        let (scheme, reference) = value.split_once(':')?;
        // Scheme must be non-empty, alphanumeric, no spaces
        if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        }
        // Reference must be non-empty
        if reference.is_empty() {
            return None;
        }
        Some((scheme, reference))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockAdapter;

    impl Adapter for MockAdapter {
        fn scheme(&self) -> &str {
            "mock"
        }

        fn resolve(&self, reference: &str) -> Result<ResourceFields> {
            Ok(ResourceFields {
                name: Some(format!("Mock: {}", reference)),
                description: Some("Resolved by mock adapter".to_string()),
                body: None,
                facts: HashMap::from([("state".to_string(), "closed".to_string())]),
            })
        }
    }

    #[test]
    fn test_registry_resolve() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(MockAdapter));

        // Both @-prefixed and plain work
        assert!(registry.is_resolvable("@mock:something"));
        assert!(registry.is_resolvable("mock:something"));
        assert!(!registry.is_resolvable("@unknown:something"));
        assert!(!registry.is_resolvable("plain-name"));

        let fields = registry.resolve("@mock:issue-42").unwrap().unwrap();
        assert_eq!(fields.name.as_deref(), Some("Mock: issue-42"));
        assert_eq!(fields.facts.get("state").map(|s| s.as_str()), Some("closed"));
    }

    #[test]
    fn test_parse_uri_edge_cases() {
        let registry = AdapterRegistry::new();

        // Valid URIs — with and without @
        assert!(registry.parse_uri("@github:tatolab/amos#15").is_some());
        assert!(registry.parse_uri("github:tatolab/amos#15").is_some());
        assert!(registry.parse_uri("@file:src/main.rs").is_some());
        assert!(registry.parse_uri("jira:PROJ-42").is_some());

        // Invalid — no colon
        assert!(registry.parse_uri("plain-name").is_none());

        // Invalid — empty scheme
        assert!(registry.parse_uri(":something").is_none());

        // Invalid — empty reference
        assert!(registry.parse_uri("gh:").is_none());

        // Invalid — scheme with spaces
        assert!(registry.parse_uri("not valid:thing").is_none());
    }

    #[test]
    fn test_batch_resolve() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(MockAdapter));

        let uris = vec!["@mock:a", "@mock:b", "@unknown:c", "plain"];
        let results = registry.resolve_batch(&uris).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.contains_key("@mock:a"));
        assert!(results.contains_key("@mock:b"));
    }

    #[test]
    fn default_trait_methods_return_empty() {
        // Adapters that don't override the new milestone/relationship
        // methods should no-op cleanly, not error — keeps every non-GitHub
        // adapter forward-compatible.
        let adapter = MockAdapter;
        assert!(adapter.list_milestones().unwrap().is_empty());
        assert!(adapter.list_nodes_in_milestone("foo").unwrap().is_empty());
        let err = adapter
            .add_relationship("a", "b", RelationshipKind::BlockedBy)
            .unwrap_err();
        assert!(format!("{}", err).contains("does not support"));
    }

    #[test]
    fn registry_list_all_milestones_aggregates_adapters() {
        let mut registry = AdapterRegistry::new();
        registry.register(Box::new(MockAdapter));
        // The default MockAdapter returns empty, so the aggregate is empty.
        assert!(registry.list_all_milestones().is_empty());
    }

    #[test]
    fn registry_add_relationship_routes_by_scheme() {
        let registry = AdapterRegistry::new();
        // No mock adapter registered — routing should fail cleanly with a
        // helpful message, not panic.
        let err = registry
            .add_relationship("@ghost:1", "@ghost:2", RelationshipKind::BlockedBy)
            .unwrap_err();
        assert!(format!("{}", err).contains("no adapter registered"));
    }
}
