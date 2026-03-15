use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

use crate::adapter::{Adapter, ResourceFields};

/// Built-in file adapter — resolves `file:` URIs to local paths.
///
/// URI format: `file:relative/path/to/file`
///
/// For text files: returns content as body.
/// For binary files (images, PDFs, etc.): returns path reference
/// so Claude Code can read them with its multimodal support.
pub struct FileAdapter {
    scan_root: PathBuf,
}

impl FileAdapter {
    pub fn new(scan_root: &Path) -> Self {
        FileAdapter {
            scan_root: scan_root.to_path_buf(),
        }
    }

    fn is_text_file(path: &Path) -> bool {
        let text_extensions = [
            "md", "txt", "rs", "py", "js", "ts", "tsx", "jsx", "json", "toml", "yaml", "yml",
            "html", "css", "sh", "bash", "zsh", "go", "java", "c", "h", "cpp", "hpp", "rb",
            "sql", "xml", "csv", "log", "conf", "cfg", "ini", "env", "gitignore", "dockerfile",
        ];

        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| text_extensions.contains(&ext.to_lowercase().as_str()))
    }
}

impl Adapter for FileAdapter {
    fn scheme(&self) -> &str {
        "file"
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        let full_path = self.scan_root.join(reference);

        if !full_path.exists() {
            bail!("file not found: {}", full_path.display());
        }

        if Self::is_text_file(&full_path) {
            let content = std::fs::read_to_string(&full_path)?;
            let ext = full_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(format!("```{}\n{}\n```", ext, content)),
            })
        } else {
            // Binary file — return path for Claude Code to read directly
            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(format!("[file: {}]", full_path.display())),
            })
        }
    }
}
