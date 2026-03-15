use anyhow::Result;
use std::collections::HashMap;

use crate::status::ManualStatus;

/// Resolved fields from an adapter. Each field is optional —
/// the adapter returns whatever it can resolve for the given URI.
#[derive(Debug, Default, Clone)]
pub struct ResourceFields {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ManualStatus>,
    pub body: Option<String>,
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
                status: Some(ManualStatus::Done),
                body: None,
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
        assert_eq!(fields.status, Some(ManualStatus::Done));
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
}
