use anyhow::{Context, Result};
use ignore::WalkBuilder;
use std::io::BufRead;
use std::path::{Path, PathBuf};

/// A raw frontmatter block extracted from a markdown file.
#[derive(Debug, Clone)]
pub struct RawBlock {
    /// The YAML content between --- delimiters.
    pub yaml: String,
    /// The markdown body after the closing ---.
    pub body: String,
    /// Source file path.
    pub source_file: PathBuf,
    /// Line number where the opening --- appears (1-based).
    pub line_number: usize,
}

/// Scan a directory for .md files and extract --- delimited blocks that contain `whoami: amos`.
pub fn scan_directory(root: &Path) -> Result<Vec<RawBlock>> {
    let mut blocks = Vec::new();

    for entry in WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip .git directory
            if name == ".git" {
                return false;
            }
            true
        })
        .build()
    {
        let entry = entry.context("failed to read directory entry")?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|ext| ext == "md") {
            if let Some(block) =
                extract_block_from_file(path).with_context(|| format!("scanning {}", path.display()))?
            {
                blocks.push(block);
            }
        }
    }

    Ok(blocks)
}

/// Extract the single amos frontmatter block from a markdown file.
/// The block must appear as the first frontmatter in the file (leading blank lines are allowed).
/// Returns `None` if the file has no amos block at the head position.
fn extract_block_from_file(path: &Path) -> Result<Option<RawBlock>> {
    let file = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    Ok(extract_block_from_reader(reader, path))
}

/// Core extraction logic shared by file and string paths.
/// Expects the amos block to be the first frontmatter block in the input.
fn extract_block_from_reader(reader: impl BufRead, source: &Path) -> Option<RawBlock> {
    let mut lines = reader.lines();
    let mut line_number: usize = 0;

    // Skip leading blank lines, find opening ---
    let opening_line = loop {
        match lines.next() {
            Some(Ok(line)) => {
                line_number += 1;
                if line.trim().is_empty() {
                    continue;
                }
                if line.trim() == "---" {
                    break line_number;
                }
                // First non-blank line is not ---, no amos block at head
                return None;
            }
            _ => return None,
        }
    };

    // Collect YAML lines until closing ---
    let mut yaml_lines = Vec::new();
    loop {
        match lines.next() {
            Some(Ok(line)) => {
                if line.trim() == "---" {
                    break;
                }
                yaml_lines.push(line);
            }
            // EOF before closing --- means no valid block
            _ => return None,
        }
    }

    let yaml_text = yaml_lines.join("\n");

    // Check for whoami: amos
    if !yaml_text.lines().any(|line| line.trim() == "whoami: amos") {
        return None;
    }

    // Read remainder as body
    let mut body_lines = Vec::new();
    for line in lines {
        if let Ok(line) = line {
            body_lines.push(line);
        }
    }
    let body = body_lines.join("\n").trim().to_string();

    Some(RawBlock {
        yaml: yaml_text,
        body,
        source_file: source.to_path_buf(),
        line_number: opening_line,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Extract an amos block from a string. Thin wrapper over `extract_block_from_reader` for tests.
    fn extract_block_from_string(content: &str, path: &Path) -> Option<RawBlock> {
        let reader = std::io::BufReader::new(content.as_bytes());
        extract_block_from_reader(reader, path)
    }

    #[test]
    fn test_single_block() {
        let content = r#"---
whoami: amos
name: task-a
description: First task
---

Some body text.
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        let block = result.expect("should find a block");
        assert!(block.yaml.contains("name: task-a"));
        assert_eq!(block.body, "Some body text.");
        assert_eq!(block.line_number, 1);
    }

    #[test]
    fn test_only_first_block_extracted() {
        let content = r#"---
whoami: amos
name: task-a
description: First
---

Body A.

---
whoami: amos
name: task-b
description: Second
dependencies:
  - up:task-a
---

Body B.
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        let block = result.expect("should find a block");
        assert!(block.yaml.contains("name: task-a"));
        // The body includes everything after the first closing ---, including the second block as raw text
        assert!(block.body.contains("Body A."));
        assert!(block.body.contains("task-b"));
    }

    #[test]
    fn test_block_without_whoami_ignored() {
        let content = r#"---
title: Not an amos block
name: some-name
---

Just regular frontmatter.
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        assert!(result.is_none());
    }

    #[test]
    fn test_non_amos_frontmatter_ignored() {
        let content = r#"---
name: claude-skill
description: A Claude Code skill, not an amos block
---

This should be ignored by amos.
"#;
        let result = extract_block_from_string(content, Path::new("skill.md"));
        assert!(result.is_none());
    }

    #[test]
    fn test_empty_body() {
        let content = r#"---
whoami: amos
name: task-a
---
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        let block = result.expect("should find a block");
        assert!(block.body.is_empty());
    }

    #[test]
    fn test_block_not_at_head_ignored() {
        let content = r#"# Some Heading

Here is some prose before the frontmatter.

---
whoami: amos
name: task-a
---

Body text.
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        assert!(result.is_none());
    }

    #[test]
    fn test_leading_blank_lines_allowed() {
        let content = "\n\n\n---\nwhoami: amos\nname: task-a\n---\n\nBody text.\n";
        let result = extract_block_from_string(content, Path::new("test.md"));
        let block = result.expect("should find a block after leading blank lines");
        assert!(block.yaml.contains("name: task-a"));
        assert_eq!(block.body, "Body text.");
        assert_eq!(block.line_number, 4);
    }

    #[test]
    fn test_body_contains_horizontal_rule() {
        let content = r#"---
whoami: amos
name: task-a
---

Some text above.

---

Some text below.
"#;
        let result = extract_block_from_string(content, Path::new("test.md"));
        let block = result.expect("should find a block");
        assert!(block.body.contains("Some text above."));
        assert!(block.body.contains("---"));
        assert!(block.body.contains("Some text below."));
    }
}
