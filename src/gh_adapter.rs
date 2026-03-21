use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::adapter::{Adapter, ResourceFields};
use crate::url_adapter::download_to_cache;

/// GitHub adapter — resolves `gh:` URIs via the `gh` CLI.
///
/// Handles GitHub-specific resources: issues, private repo files.
/// For plain public URLs, use the `url:` adapter instead.
///
/// URI formats:
/// - `gh:15` — issue #15 in the current repo
/// - `gh:owner/repo#15` — issue #15 in a specific repo
/// - `gh:owner/repo/path/to/file.png` — file in a repo (uses gh for private access)
pub struct GhAdapter {
    default_repo: Option<String>,
}

impl GhAdapter {
    pub fn new(default_repo: Option<String>) -> Self {
        GhAdapter { default_repo }
    }

    fn parse_ref(&self, reference: &str) -> Result<(Option<String>, u64)> {
        if let Some((repo, num_str)) = reference.split_once('#') {
            let num: u64 = num_str
                .parse()
                .with_context(|| format!("invalid issue number in '{}'", reference))?;
            Ok((Some(repo.to_string()), num))
        } else {
            let num: u64 = reference
                .parse()
                .with_context(|| format!("invalid issue reference '{}' — expected number or owner/repo#number", reference))?;
            Ok((self.default_repo.clone(), num))
        }
    }

    fn fetch_issue(&self, repo: Option<&str>, number: u64) -> Result<IssueData> {
        let mut cmd = Command::new("gh");
        cmd.args(["issue", "view", &number.to_string()]);
        cmd.args(["--json", "title,body,state,labels"]);

        if let Some(r) = repo {
            cmd.args(["--repo", r]);
        }

        let output = cmd
            .output()
            .context("failed to run 'gh issue view' — is gh CLI installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("gh issue view failed: {}", stderr.trim());
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("parsing gh output")?;

        Ok(IssueData {
            title: json["title"].as_str().unwrap_or("").to_string(),
            body: json["body"].as_str().unwrap_or("").to_string(),
            state: json["state"].as_str().unwrap_or("OPEN").to_string(),
            labels: json["labels"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l["name"].as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// Resolve a file in a GitHub repo. Uses `gh` for auth (works with private repos).
    fn resolve_file(&self, reference: &str) -> Result<ResourceFields> {
        let parts: Vec<&str> = reference.splitn(3, '/').collect();
        if parts.len() < 3 {
            bail!(
                "invalid file reference '{}' — expected owner/repo/path",
                reference
            );
        }
        let repo = format!("{}/{}", parts[0], parts[1]);
        let file_path = parts[2];

        let image_extensions = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"];
        let is_image = image_extensions
            .iter()
            .any(|ext| file_path.to_lowercase().ends_with(ext));

        // Use gh api for private repo access
        let raw_url = format!(
            "https://raw.githubusercontent.com/{}/HEAD/{}",
            repo, file_path
        );

        if is_image {
            let filename = file_path.rsplit('/').next().unwrap_or(file_path);
            let local_path = download_to_cache(&raw_url, filename)?;
            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(format!("![{}]({})", filename, local_path.display())),
            })
        } else {
            let mut cmd = Command::new("gh");
            cmd.args(["api", &raw_url, "--method", "GET"]);

            let output = cmd.output().context("failed to run gh api")?;

            if !output.status.success() {
                return Ok(ResourceFields {
                    name: None,
                    description: None,
                    status: None,
                    body: Some(format!(
                        "[GitHub file: {}/{}](https://github.com/{}/blob/HEAD/{})",
                        repo, file_path, repo, file_path
                    )),
                });
            }

            let content = String::from_utf8_lossy(&output.stdout).to_string();
            let ext = file_path.rsplit('.').next().unwrap_or("");

            Ok(ResourceFields {
                name: None,
                description: None,
                status: None,
                body: Some(format!("```{}\n{}\n```", ext, content)),
            })
        }
    }

    fn fetch_issues_batch(
        &self,
        repo: Option<&str>,
        numbers: &[u64],
    ) -> Result<HashMap<u64, IssueData>> {
        if numbers.is_empty() {
            return Ok(HashMap::new());
        }

        let mut results = HashMap::new();
        for &num in numbers {
            match self.fetch_issue(repo, num) {
                Ok(data) => {
                    results.insert(num, data);
                }
                Err(e) => {
                    eprintln!("warning: failed to fetch issue #{}: {}", num, e);
                }
            }
        }
        Ok(results)
    }
}

struct IssueData {
    title: String,
    body: String,
    state: String,
    labels: Vec<String>,
}

impl IssueData {
    fn to_status(&self) -> Option<String> {
        match self.state.as_str() {
            "CLOSED" => Some("closed".to_string()),
            "OPEN" => {
                // Surface status-like labels from the external system
                for label in &self.labels {
                    let lower = label.to_lowercase();
                    if lower == "in-progress" || lower == "in progress" {
                        return Some(label.clone());
                    }
                }
                None // open with no status label = no explicit status
            }
            other => Some(other.to_lowercase()),
        }
    }
}

impl Adapter for GhAdapter {
    fn scheme(&self) -> &str {
        "github"
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        // Detect file references: contains `/` but no `#`, and doesn't parse as a number
        if reference.contains('/') && !reference.contains('#') && reference.parse::<u64>().is_err()
        {
            return self.resolve_file(reference);
        }

        let (repo, number) = self.parse_ref(reference)?;
        let issue = self.fetch_issue(repo.as_deref(), number)?;

        // Download any images embedded in the issue body to local cache
        let body = if issue.body.is_empty() {
            None
        } else {
            Some(localize_markdown_images(&issue.body))
        };

        Ok(ResourceFields {
            name: Some(issue.title.clone()),
            description: Some(issue.title.clone()),
            status: issue.to_status(),
            body,
        })
    }

    fn notify(&self, reference: &str, message: &str) -> Result<()> {
        // Only issue references, not file references
        if reference.contains('/') && !reference.contains('#') {
            return Ok(());
        }

        let (repo, number) = self.parse_ref(reference)?;
        let repo_str = repo.as_deref().unwrap_or("");

        let mut cmd = Command::new("gh");
        cmd.args(["issue", "comment", &number.to_string()]);
        cmd.args(["--body", message]);
        if !repo_str.is_empty() {
            cmd.args(["--repo", repo_str]);
        }

        let output = cmd.output().context("failed to comment on issue")?;
        if output.status.success() {
            eprintln!("amos: commented on {}#{}", repo_str, number);
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("amos: failed to comment on {}#{}: {}", repo_str, number, stderr.trim());
        }

        Ok(())
    }

    fn resolve_batch(&self, references: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        let mut by_repo: HashMap<Option<String>, Vec<(String, u64)>> = HashMap::new();
        for &reference in references {
            let (repo, number) = self.parse_ref(reference)?;
            by_repo
                .entry(repo)
                .or_default()
                .push((reference.to_string(), number));
        }

        let mut results = HashMap::new();
        for (repo, entries) in by_repo {
            let numbers: Vec<u64> = entries.iter().map(|(_, n)| *n).collect();
            let issues = self.fetch_issues_batch(repo.as_deref(), &numbers)?;

            for (reference, number) in &entries {
                if let Some(issue) = issues.get(number) {
                    results.insert(
                        reference.clone(),
                        ResourceFields {
                            name: Some(issue.title.clone()),
                            description: Some(issue.title.clone()),
                            status: issue.to_status(),
                            body: if issue.body.is_empty() {
                                None
                            } else {
                                Some(issue.body.clone())
                            },
                        },
                    );
                }
            }
        }

        Ok(results)
    }
}

/// Scan markdown for `![alt](url)` with remote URLs, download to local cache,
/// replace URLs with local paths so Claude Code reads images directly.
fn localize_markdown_images(body: &str) -> String {
    let mut result = String::with_capacity(body.len());

    for line in body.lines() {
        if line.contains("![") && line.contains("](http") {
            result.push_str(&localize_images_in_line(line));
        } else {
            result.push_str(line);
        }
        result.push('\n');
    }

    if result.ends_with('\n') && !body.ends_with('\n') {
        result.pop();
    }

    result
}

fn localize_images_in_line(line: &str) -> String {
    let mut result = String::new();
    let mut remaining = line;

    while let Some(start) = remaining.find("![") {
        result.push_str(&remaining[..start]);

        let after_bang = &remaining[start + 2..];
        if let Some(close_bracket) = after_bang.find("](") {
            let alt = &after_bang[..close_bracket];
            let after_paren = &after_bang[close_bracket + 2..];

            if let Some(close_paren) = after_paren.find(')') {
                let url = &after_paren[..close_paren];

                if url.starts_with("http") {
                    let filename = url.rsplit('/').next().unwrap_or("image.png");
                    match download_to_cache(url, filename) {
                        Ok(local) => {
                            result.push_str(&format!("![{}]({})", alt, local.display()));
                        }
                        Err(_) => {
                            result.push_str(&format!("![{}]({})", alt, url));
                        }
                    }
                } else {
                    result.push_str(&format!("![{}]({})", alt, url));
                }

                remaining = &after_paren[close_paren + 1..];
                continue;
            }
        }

        result.push_str("![");
        remaining = after_bang;
    }

    result.push_str(remaining);
    result
}
