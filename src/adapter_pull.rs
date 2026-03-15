use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::external_adapter::ExternalAdapter;
use crate::parser::Node;

/// Trust configuration for adapter auto-pull.
pub struct TrustConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

impl TrustConfig {
    /// Load trust config from .amosrc.toml. If no file or no [trust] section, allow nothing.
    pub fn load(scan_root: &Path) -> Self {
        let config_path = scan_root.join(".amosrc.toml");
        let content = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(_) => return TrustConfig::default(),
        };

        let config = match content.parse::<toml::Table>() {
            Ok(c) => c,
            Err(_) => return TrustConfig::default(),
        };

        let trust = match config.get("trust").and_then(|v| v.as_table()) {
            Some(t) => t,
            None => return TrustConfig::default(),
        };

        let allow = trust
            .get("allow")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let deny = trust
            .get("deny")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        TrustConfig { allow, deny }
    }

    /// Check if a source URI is trusted.
    /// Deny rules take precedence. Then allow rules are checked.
    /// If no rules are configured, nothing is trusted (safe default).
    pub fn is_trusted(&self, source: &str) -> bool {
        // Deny takes precedence
        for pattern in &self.deny {
            if matches_glob(source, pattern) {
                return false;
            }
        }

        // Check allow
        for pattern in &self.allow {
            if matches_glob(source, pattern) {
                return true;
            }
        }

        // No matching rule — not trusted
        false
    }
}

impl Default for TrustConfig {
    fn default() -> Self {
        TrustConfig {
            allow: Vec::new(),
            deny: Vec::new(),
        }
    }
}

/// Simple glob matching: `*` at the end matches anything.
/// `@github:openclaw/*` matches `@github:openclaw/amos-adapters#figma`.
fn matches_glob(value: &str, pattern: &str) -> bool {
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        value == pattern
    }
}

/// Collect all adapter declarations from parsed nodes.
/// Returns a deduplicated map of scheme → source URI.
pub fn collect_adapter_declarations(nodes: &[Node]) -> HashMap<String, String> {
    let mut adapters = HashMap::new();
    for node in nodes {
        for (scheme, source) in &node.adapters {
            adapters.entry(scheme.clone()).or_insert_with(|| source.clone());
        }
    }
    adapters
}

/// Pull an adapter from a GitHub source URI and return the local path.
///
/// Source format: `@github:owner/repo#path/to/adapter`
///
/// The adapter is a directory or single file in the repo. We clone/fetch
/// just the file and cache it locally.
pub fn pull_adapter(source: &str) -> Result<PathBuf> {
    let cache_dir = std::env::temp_dir()
        .join("amos-cache")
        .join("adapters");
    std::fs::create_dir_all(&cache_dir).context("creating adapter cache dir")?;

    let reference = source
        .strip_prefix("@github:")
        .or_else(|| source.strip_prefix("github:"))
        .ok_or_else(|| anyhow::anyhow!("unsupported adapter source: {} — only @github: sources supported", source))?;

    let (repo, path) = reference
        .split_once('#')
        .ok_or_else(|| anyhow::anyhow!("invalid adapter source '{}' — expected @github:owner/repo#path", source))?;

    // Cache key based on repo + path
    let cache_key = format!("{}_{}", repo.replace('/', "_"), path.replace('/', "_"));
    let adapter_dir = cache_dir.join(&cache_key);

    // Find the executable in the cached adapter
    if adapter_dir.exists() {
        return find_executable(&adapter_dir, path);
    }

    std::fs::create_dir_all(&adapter_dir).context("creating adapter dir")?;

    // Use gh to download the file/directory
    // Try single file first
    let filename = path.rsplit('/').next().unwrap_or(path);
    let output_path = adapter_dir.join(filename);

    let mut cmd = Command::new("gh");
    cmd.args([
        "api",
        &format!("repos/{}/contents/{}", repo, path),
        "--jq", ".download_url",
    ]);

    let output = cmd.output().context("failed to query GitHub for adapter")?;

    if output.status.success() {
        let download_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !download_url.is_empty() && download_url != "null" {
            // Download the file
            let dl_output = Command::new("curl")
                .args(["-sL", "-o"])
                .arg(&output_path)
                .arg(&download_url)
                .output()
                .context("failed to download adapter")?;

            if !dl_output.status.success() {
                bail!("failed to download adapter from {}", download_url);
            }

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&output_path)?.permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&output_path, perms)?;
            }

            return Ok(output_path);
        }
    }

    // If single file failed, try as a directory — list contents and download all
    let mut cmd = Command::new("gh");
    cmd.args([
        "api",
        &format!("repos/{}/contents/{}", repo, path),
        "--jq", ".[].download_url",
    ]);

    let output = cmd.output().context("failed to list adapter directory")?;

    if output.status.success() {
        let urls = String::from_utf8_lossy(&output.stdout);
        for url in urls.lines() {
            let url = url.trim();
            if url.is_empty() || url == "null" {
                continue;
            }
            let fname = url.rsplit('/').next().unwrap_or("file");
            let fpath = adapter_dir.join(fname);
            Command::new("curl")
                .args(["-sL", "-o"])
                .arg(&fpath)
                .arg(url)
                .output()
                .context("downloading adapter file")?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(meta) = std::fs::metadata(&fpath) {
                    let mut perms = meta.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&fpath, perms);
                }
            }
        }
        return find_executable(&adapter_dir, path);
    }

    bail!("failed to pull adapter from {}", source);
}

/// Find the main executable in a pulled adapter directory.
/// Looks for common patterns: resolve, resolve.py, resolve.sh, index.js, etc.
fn find_executable(dir: &Path, original_path: &str) -> Result<PathBuf> {
    let basename = original_path.rsplit('/').next().unwrap_or(original_path);

    // Check for the file directly (single-file adapter)
    let direct = dir.join(basename);
    if direct.is_file() {
        return Ok(direct);
    }

    // Common executable names
    let candidates = [
        "resolve", "resolve.py", "resolve.sh", "resolve.js", "resolve.ts",
        "index.js", "index.py", "main.py", "main.sh",
    ];

    for candidate in &candidates {
        let path = dir.join(candidate);
        if path.is_file() {
            return Ok(path);
        }
    }

    // Just return the first file we find
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_file() {
                return Ok(entry.path());
            }
        }
    }

    bail!("no executable found in adapter at {}", dir.display());
}

/// Build external adapters from node declarations, respecting trust config.
/// Returns a list of (scheme, ExternalAdapter) pairs ready to register.
pub fn build_declared_adapters(
    nodes: &[Node],
    trust: &TrustConfig,
    builtin_schemes: &[&str],
) -> Vec<(String, ExternalAdapter)> {
    let declarations = collect_adapter_declarations(nodes);
    let mut adapters = Vec::new();

    for (scheme, source) in declarations {
        // Skip built-in schemes
        if builtin_schemes.contains(&scheme.as_str()) {
            continue;
        }

        // Check trust
        if !trust.is_trusted(&source) {
            eprintln!(
                "amos: skipping untrusted adapter '{}' from {} (add to [trust].allow in .amosrc.toml)",
                scheme, source
            );
            continue;
        }

        // Pull the adapter
        match pull_adapter(&source) {
            Ok(executable) => {
                let cmd = executable_command(&executable);
                eprintln!("amos: pulled adapter '{}' from {}", scheme, source);
                adapters.push((scheme.clone(), ExternalAdapter::new(&scheme, &cmd)));
            }
            Err(e) => {
                eprintln!("amos: failed to pull adapter '{}': {}", scheme, e);
            }
        }
    }

    adapters
}

/// Determine the command to run an adapter executable.
/// Python files get `python3`, JS files get `node`, etc.
fn executable_command(path: &Path) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let path_str = path.to_string_lossy();

    match ext {
        "py" => format!("python3 {}", path_str),
        "js" => format!("node {}", path_str),
        "ts" => format!("npx tsx {}", path_str),
        "sh" | "bash" => format!("bash {}", path_str),
        _ => path_str.to_string(), // Assume executable directly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trust_config_default_denies() {
        let trust = TrustConfig::default();
        assert!(!trust.is_trusted("@github:someone/repo#adapter"));
    }

    #[test]
    fn test_trust_config_allow() {
        let trust = TrustConfig {
            allow: vec!["@github:openclaw/*".to_string()],
            deny: Vec::new(),
        };
        assert!(trust.is_trusted("@github:openclaw/amos-adapters#figma"));
        assert!(!trust.is_trusted("@github:random/repo#adapter"));
    }

    #[test]
    fn test_trust_config_deny_overrides_allow() {
        let trust = TrustConfig {
            allow: vec!["@github:openclaw/*".to_string()],
            deny: vec!["@github:openclaw/untrusted*".to_string()],
        };
        assert!(trust.is_trusted("@github:openclaw/amos-adapters#figma"));
        assert!(!trust.is_trusted("@github:openclaw/untrusted-repo#adapter"));
    }

    #[test]
    fn test_matches_glob() {
        assert!(matches_glob("@github:openclaw/foo", "@github:openclaw/*"));
        assert!(!matches_glob("@github:other/foo", "@github:openclaw/*"));
        assert!(matches_glob("@github:openclaw/foo", "@github:openclaw/foo"));
        assert!(!matches_glob("@github:openclaw/foo", "@github:openclaw/bar"));
    }
}
