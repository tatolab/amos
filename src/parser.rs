use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

use crate::scanner::RawBlock;

/// A parsed amos node.
#[derive(Debug, Clone)]
pub struct Node {
    /// Unique node identifier.
    pub name: String,
    /// Plain-language description of the work.
    pub description: Option<String>,
    /// Upstream dependencies (nodes this depends on).
    pub upstream: Vec<String>,
    /// Downstream dependents (nodes that depend on this).
    pub downstream: Vec<String>,
    /// Context references (local files, @github:, @url:).
    pub context: Vec<ContextRef>,
    /// Source file where this node was defined.
    pub source_file: PathBuf,
    /// Line number of the opening --- (1-based).
    pub line_number: usize,
    /// Markdown body after the frontmatter block.
    pub body: String,
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
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    context: Vec<String>,
}

/// Parse a raw block into a Node.
pub fn parse_block(block: &RawBlock) -> Result<Node> {
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

    let mut upstream = Vec::new();
    let mut downstream = Vec::new();

    for dep in &frontmatter.dependencies {
        if let Some(name) = dep.strip_prefix("up:") {
            upstream.push(name.trim().to_string());
        } else if let Some(name) = dep.strip_prefix("down:") {
            downstream.push(name.trim().to_string());
        } else {
            bail!(
                "invalid dependency '{}' at {}:{} — must start with 'up:' or 'down:'",
                dep,
                block.source_file.display(),
                block.line_number
            );
        }
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

    Ok(Node {
        name: frontmatter.name,
        description: frontmatter.description,
        upstream,
        downstream,
        context,
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
    fn test_parse_dependencies() {
        let block = make_block(
            r#"whoami: amos
name: task-b
dependencies:
  - up:task-a
  - down:task-c"#,
            "",
        );
        let node = parse_block(&block).unwrap();
        assert_eq!(node.upstream, vec!["task-a"]);
        assert_eq!(node.downstream, vec!["task-c"]);
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

    #[test]
    fn test_invalid_dependency_prefix() {
        let block = make_block(
            r#"whoami: amos
name: task-a
dependencies:
  - invalid-dep"#,
            "",
        );
        assert!(parse_block(&block).is_err());
    }

    #[test]
    fn test_minimal_node() {
        let block = make_block("whoami: amos\nname: task-a", "");
        let node = parse_block(&block).unwrap();
        assert_eq!(node.name, "task-a");
        assert!(node.description.is_none());
        assert!(node.upstream.is_empty());
        assert!(node.downstream.is_empty());
        assert!(node.context.is_empty());
    }
}
