use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::scanner::RawBlock;

/// A parsed amos node.
#[derive(Debug, Clone)]
pub struct Node {
    /// Unique node identifier.
    pub name: String,
    /// Plain-language description of the work.
    pub description: Option<String>,

    // --- Typed relationships. ---
    /// Nodes that must complete before this one can start.
    pub blocked_by: Vec<String>,
    /// Nodes that can't start until this one is done.
    pub blocks: Vec<String>,
    /// Soft associations — no ordering or gating.
    pub related_to: Vec<String>,
    /// This node duplicates another (that one is canonical).
    pub duplicates: Option<String>,
    /// This node has been replaced by another.
    pub superseded_by: Option<String>,

    // --- Attributes. ---
    /// Free-form tags for filtering.
    pub labels: Vec<String>,
    /// Priority bucket.
    pub priority: Option<Priority>,

    // --- Existing fields. ---
    /// Context references (local files, @github:, @url:).
    pub context: Vec<ContextRef>,
    /// Adapter declarations — scheme → source URI for auto-pull.
    pub adapters: HashMap<String, String>,
    /// Source file where this node was defined.
    pub source_file: PathBuf,
    /// Line number of the opening --- (1-based).
    pub line_number: usize,
    /// Markdown body after the frontmatter block.
    pub body: String,
}

/// Priority bucket, ordered from highest (P0) to lowest (P3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    P0,
    P1,
    P2,
    P3,
}

impl std::str::FromStr for Priority {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "p0" | "0" => Ok(Priority::P0),
            "p1" | "1" => Ok(Priority::P1),
            "p2" | "2" => Ok(Priority::P2),
            "p3" | "3" => Ok(Priority::P3),
            other => Err(format!(
                "invalid priority '{}'; expected p0, p1, p2, or p3",
                other
            )),
        }
    }
}

/// A context reference pointing to a file or URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRef {
    /// Local file path relative to repo root.
    LocalFile(String),
    /// GitHub file reference: org/repo[#path][@ref].
    GitHub {
        owner_repo: String,
        path: Option<String>,
        git_ref: Option<String>,
    },
    /// Arbitrary URL.
    Url(String),
}

/// Intermediate deserialization struct for YAML frontmatter.
#[derive(Debug, Deserialize)]
struct RawFrontmatter {
    whoami: String,
    name: String,
    description: Option<String>,

    // Typed relationships.
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    blocks: Vec<String>,
    #[serde(default)]
    related_to: Vec<String>,
    #[serde(default)]
    duplicates: Option<String>,
    #[serde(default)]
    superseded_by: Option<String>,

    // Attributes.
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    priority: Option<String>,

    #[serde(default)]
    context: Vec<String>,
    #[serde(default)]
    adapters: HashMap<String, String>,
}

/// Parse a raw block into a Node.
pub fn parse_block(block: &RawBlock) -> Result<Node> {
    // Legacy-format detection: the old `dependencies:` list with up:/down:
    // prefixes and the in-frontmatter `status:` field both moved out in
    // favor of typed edges and `.amos-status`. Point users to the migration
    // tool rather than silently dropping fields.
    let raw: serde_yaml::Value = serde_yaml::from_str(&block.yaml).with_context(|| {
        format!(
            "parsing YAML at {}:{}",
            block.source_file.display(),
            block.line_number
        )
    })?;
    if let Some(map) = raw.as_mapping() {
        if map.contains_key("dependencies") {
            bail!(
                "legacy `dependencies:` field in {}:{} — run `amos migrate` to convert to typed edges (blocked_by:/blocks:)",
                block.source_file.display(),
                block.line_number
            );
        }
        if map.contains_key("status") {
            bail!(
                "legacy `status:` field in {}:{} — run `amos migrate` to move status entries into .amos-status",
                block.source_file.display(),
                block.line_number
            );
        }
    }

    let frontmatter: RawFrontmatter =
        serde_yaml::from_str(&block.yaml).with_context(|| {
            format!(
                "parsing YAML at {}:{}",
                block.source_file.display(),
                block.line_number
            )
        })?;

    if frontmatter.whoami != "amos" {
        bail!(
            "whoami is '{}', expected 'amos' at {}:{}",
            frontmatter.whoami,
            block.source_file.display(),
            block.line_number
        );
    }

    if frontmatter.name.is_empty() {
        bail!(
            "empty name at {}:{}",
            block.source_file.display(),
            block.line_number
        );
    }

    let context = frontmatter
        .context
        .iter()
        .map(|s| parse_context_ref(s))
        .collect::<Result<Vec<_>>>()
        .with_context(|| {
            format!(
                "parsing context at {}:{}",
                block.source_file.display(),
                block.line_number
            )
        })?;

    let priority = frontmatter
        .priority
        .as_deref()
        .map(|s| {
            s.parse::<Priority>().map_err(|e| {
                anyhow::anyhow!(
                    "{} at {}:{}",
                    e,
                    block.source_file.display(),
                    block.line_number
                )
            })
        })
        .transpose()?;

    Ok(Node {
        name: frontmatter.name,
        description: frontmatter.description,
        blocked_by: frontmatter.blocked_by,
        blocks: frontmatter.blocks,
        related_to: frontmatter.related_to,
        duplicates: frontmatter.duplicates,
        superseded_by: frontmatter.superseded_by,
        labels: frontmatter.labels,
        priority,
        context,
        adapters: frontmatter.adapters,
        source_file: block.source_file.clone(),
        line_number: block.line_number,
        body: block.body.clone(),
    })
}

/// Parse a context reference string.
fn parse_context_ref(s: &str) -> Result<ContextRef> {
    if let Some(url) = s.strip_prefix("@url:") {
        Ok(ContextRef::Url(url.to_string()))
    } else if let Some(github) = s.strip_prefix("@github:") {
        parse_github_ref(github)
    } else {
        Ok(ContextRef::LocalFile(s.to_string()))
    }
}

/// Parse a GitHub reference: owner/repo[@ref]#path
fn parse_github_ref(s: &str) -> Result<ContextRef> {
    let (repo_part, path) = if let Some((repo, path)) = s.split_once('#') {
        (repo, Some(path.to_string()))
    } else {
        (s, None)
    };

    let (owner_repo, git_ref) = if let Some((repo, r)) = repo_part.split_once('@') {
        (repo.to_string(), Some(r.to_string()))
    } else {
        (repo_part.to_string(), None)
    };

    Ok(ContextRef::GitHub {
        owner_repo,
        path,
        git_ref,
    })
}

/// Parse all blocks into nodes.
pub fn parse_blocks(blocks: &[RawBlock]) -> Result<Vec<Node>> {
    blocks.iter().map(parse_block).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(yaml: &str, body: &str) -> RawBlock {
        RawBlock {
            yaml: yaml.to_string(),
            body: body.to_string(),
            source_file: PathBuf::from("test.md"),
            line_number: 1,
        }
    }

    #[test]
    fn test_parse_basic_node() {
        let block = make_block(
            r#"whoami: amos
name: task-a
description: First task"#,
            "Some notes.",
        );
        let node = parse_block(&block).unwrap();
        assert_eq!(node.name, "task-a");
        assert_eq!(node.description.as_deref(), Some("First task"));
        assert_eq!(node.body, "Some notes.");
    }

    #[test]
    fn test_legacy_dependencies_errors_with_migrate_hint() {
        let block = make_block(
            r#"whoami: amos
name: task-b
dependencies:
  - up:task-a
  - down:task-c"#,
            "",
        );
        let err = parse_block(&block).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("amos migrate"), "expected migrate hint: {}", msg);
    }

    #[test]
    fn test_legacy_status_errors_with_migrate_hint() {
        let block = make_block(
            r#"whoami: amos
name: task-a
status: pending"#,
            "",
        );
        let err = parse_block(&block).unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("amos migrate"), "expected migrate hint: {}", msg);
    }

    #[test]
    fn test_parse_context_refs() {
        let block = make_block(
            r#"whoami: amos
name: task-a
context:
  - src/main.rs
  - "@github:tatolab/streamlib#libs/streamlib/src/lib.rs"
  - "@github:tatolab/streamlib@main#docs/arch.md"
  - "@url:https://example.com/docs""#,
            "",
        );
        let node = parse_block(&block).unwrap();
        assert_eq!(node.context.len(), 4);
        assert_eq!(node.context[0], ContextRef::LocalFile("src/main.rs".to_string()));
        assert!(matches!(&node.context[1], ContextRef::GitHub { owner_repo, path, git_ref }
            if owner_repo == "tatolab/streamlib"
            && path.as_deref() == Some("libs/streamlib/src/lib.rs")
            && git_ref.is_none()
        ));
        assert!(matches!(&node.context[2], ContextRef::GitHub { owner_repo, path, git_ref }
            if owner_repo == "tatolab/streamlib"
            && path.as_deref() == Some("docs/arch.md")
            && git_ref.as_deref() == Some("main")
        ));
        assert_eq!(
            node.context[3],
            ContextRef::Url("https://example.com/docs".to_string())
        );
    }

    // Legacy `dependencies:` now errors with a migrate hint; see
    // test_legacy_dependencies_errors_with_migrate_hint for the new behavior.

    #[test]
    fn test_minimal_node() {
        let block = make_block("whoami: amos\nname: task-a", "");
        let node = parse_block(&block).unwrap();
        assert_eq!(node.name, "task-a");
        assert!(node.description.is_none());
        assert!(node.context.is_empty());
        assert!(node.blocked_by.is_empty());
        assert!(node.blocks.is_empty());
        assert!(node.related_to.is_empty());
        assert!(node.labels.is_empty());
        assert!(node.priority.is_none());
    }

    #[test]
    fn test_parse_typed_relationships() {
        let block = make_block(
            r#"whoami: amos
name: "@github:tatolab/streamlib#326"
blocked_by:
  - "@github:tatolab/streamlib#325"
  - "@github:tatolab/streamlib#369"
blocks:
  - "@github:tatolab/streamlib#999"
related_to:
  - "@github:tatolab/streamlib#888"
duplicates: "@github:tatolab/streamlib#777"
superseded_by: "@github:tatolab/streamlib#555""#,
            "",
        );
        let node = parse_block(&block).unwrap();
        assert_eq!(node.blocked_by.len(), 2);
        assert_eq!(node.blocked_by[0], "@github:tatolab/streamlib#325");
        assert_eq!(node.blocked_by[1], "@github:tatolab/streamlib#369");
        assert_eq!(node.blocks, vec!["@github:tatolab/streamlib#999"]);
        assert_eq!(node.related_to, vec!["@github:tatolab/streamlib#888"]);
        assert_eq!(
            node.duplicates.as_deref(),
            Some("@github:tatolab/streamlib#777")
        );
        assert_eq!(
            node.superseded_by.as_deref(),
            Some("@github:tatolab/streamlib#555")
        );
    }

    #[test]
    fn test_parse_attributes() {
        let block = make_block(
            r#"whoami: amos
name: task-a
labels:
  - refactor
  - gpu
priority: p2"#,
            "",
        );
        let node = parse_block(&block).unwrap();
        assert_eq!(node.labels, vec!["refactor", "gpu"]);
        assert_eq!(node.priority, Some(Priority::P2));
    }

    #[test]
    fn test_parse_priority_alternate_forms() {
        for (raw, expected) in [
            ("p0", Priority::P0),
            ("P1", Priority::P1),
            ("2", Priority::P2),
            ("P3", Priority::P3),
        ] {
            let block = make_block(
                &format!("whoami: amos\nname: t\npriority: \"{}\"", raw),
                "",
            );
            let node = parse_block(&block).unwrap();
            assert_eq!(node.priority, Some(expected), "failed for raw={}", raw);
        }
    }

    #[test]
    fn test_parse_invalid_priority_errors() {
        let block = make_block(
            "whoami: amos\nname: t\npriority: urgent",
            "",
        );
        assert!(parse_block(&block).is_err());
    }

}
