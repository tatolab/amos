use crate::adapter::AdapterRegistry;

/// Resolve `@scheme:reference` lines in a node body.
///
/// Lines starting with `@` followed by a registered scheme are resolved
/// through the adapter registry. All other lines pass through as-is.
///
/// Returns the body with references expanded inline.
pub fn resolve_body(body: &str, registry: &AdapterRegistry) -> String {
    let mut resolved_lines = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();

        if let Some(reference) = parse_at_reference(trimmed) {
            if registry.is_resolvable(reference) {
                match registry.resolve(reference) {
                    Some(Ok(fields)) => {
                        // Inline whatever the adapter returned
                        if let Some(desc) = &fields.description {
                            resolved_lines.push(format!("> {}", desc));
                            resolved_lines.push(String::new());
                        }
                        if let Some(body) = &fields.body {
                            resolved_lines.push(body.clone());
                        }
                    }
                    Some(Err(e)) => {
                        resolved_lines.push(format!("*Failed to resolve `{}`: {}*", trimmed, e));
                    }
                    None => {
                        // Scheme not registered — pass through
                        resolved_lines.push(line.to_string());
                    }
                }
            } else {
                // Not a registered scheme — pass through as-is
                resolved_lines.push(line.to_string());
            }
        } else {
            resolved_lines.push(line.to_string());
        }
    }

    resolved_lines.join("\n")
}

/// Parse an `@scheme:reference` line. Returns the full `scheme:reference` part.
///
/// Rules:
/// - Line must start with `@` (after optional whitespace)
/// - Must contain `:` to separate scheme from reference
/// - Scheme must be alphanumeric (no spaces)
fn parse_at_reference(line: &str) -> Option<&str> {
    let stripped = line.strip_prefix('@')?;
    let (scheme, reference) = stripped.split_once(':')?;

    // Scheme must be non-empty, alphanumeric
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }

    // Reference must be non-empty
    if reference.is_empty() {
        return None;
    }

    Some(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_at_reference() {
        assert_eq!(parse_at_reference("@gh:tatolab/amos#13"), Some("gh:tatolab/amos#13"));
        assert_eq!(parse_at_reference("@file:src/main.rs"), Some("file:src/main.rs"));
        assert_eq!(parse_at_reference("@jira:PROJ-42"), Some("jira:PROJ-42"));

        // Not references
        assert_eq!(parse_at_reference("regular line"), None);
        assert_eq!(parse_at_reference("@"), None);
        assert_eq!(parse_at_reference("@:nothing"), None);
        assert_eq!(parse_at_reference("@scheme:"), None);
        assert_eq!(parse_at_reference("email@example.com"), None);
    }

    #[test]
    fn test_resolve_body_passthrough() {
        let registry = AdapterRegistry::new();
        let body = "This is regular text.\n\nNo references here.";
        let result = resolve_body(body, &registry);
        assert_eq!(result, body);
    }

    #[test]
    fn test_resolve_body_unregistered_scheme() {
        let registry = AdapterRegistry::new();
        let body = "Some text.\n\n@unknown:something\n\nMore text.";
        let result = resolve_body(body, &registry);
        // Unregistered scheme passes through as-is
        assert!(result.contains("@unknown:something"));
    }
}
