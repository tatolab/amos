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
        cmd.args(["--json", "title,body,state,labels,comments,milestone"]);

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

        let comments = json["comments"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let author = c["author"]["login"].as_str().unwrap_or("unknown");
                        let created = c["createdAt"]
                            .as_str()
                            .and_then(|s| s.get(..10))
                            .unwrap_or("");
                        let body = c["body"].as_str()?;
                        Some(Comment {
                            author: author.to_string(),
                            date: created.to_string(),
                            body: body.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let milestone = json["milestone"]["title"].as_str().map(String::from);

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
            comments,
            milestone,
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
                facts: HashMap::new(),
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
                    facts: HashMap::new(),
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
                facts: HashMap::new(),
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

        // Single paginated API call — massively faster than N individual
        // `gh issue view` calls. A scan over 80 issues goes from ~2 minutes
        // to a couple of seconds. We fetch state=all so both open and
        // closed issues land in the map; the caller filters as needed.
        let repo_str = match repo.or(self.default_repo.as_deref()) {
            Some(r) => r.to_string(),
            None => return self.fetch_issues_batch_sequential(repo, numbers),
        };

        let mut cmd = Command::new("gh");
        cmd.args([
            "api",
            "--paginate",
            "-H",
            "Accept: application/vnd.github+json",
            &format!("repos/{}/issues?state=all&per_page=100", repo_str),
        ]);
        let output = match cmd.output() {
            Ok(o) if o.status.success() => o,
            _ => {
                return self.fetch_issues_batch_sequential(repo, numbers);
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let issues = parse_issue_pages(&stdout).unwrap_or_default();

        let wanted: std::collections::HashSet<u64> = numbers.iter().copied().collect();
        let mut results = HashMap::new();
        for entry in issues {
            let Some(number) = entry.get("number").and_then(|v| v.as_u64()) else { continue };
            if !wanted.contains(&number) {
                continue;
            }
            if entry.get("pull_request").is_some() {
                continue;
            }

            let title = entry
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let body = entry
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let state = entry
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("OPEN")
                .to_uppercase();
            let labels = entry
                .get("labels")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|l| l.get("name").and_then(|v| v.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let milestone = entry
                .get("milestone")
                .and_then(|m| m.get("title"))
                .and_then(|v| v.as_str())
                .map(String::from);

            results.insert(
                number,
                IssueData {
                    title,
                    body,
                    state,
                    labels,
                    comments: Vec::new(),
                    milestone,
                },
            );
        }

        // Anything not found in the bulk response falls back to per-issue
        // fetch.
        let missing: Vec<u64> = numbers
            .iter()
            .copied()
            .filter(|n| !results.contains_key(n))
            .collect();
        if !missing.is_empty() {
            let extra = self.fetch_issues_batch_sequential(repo, &missing)?;
            results.extend(extra);
        }

        Ok(results)
    }

    /// Original per-issue loop, kept as the fallback path.
    fn fetch_issues_batch_sequential(
        &self,
        repo: Option<&str>,
        numbers: &[u64],
    ) -> Result<HashMap<u64, IssueData>> {
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

/// Parse the concatenated JSON that `gh api --paginate` emits. Normally a
/// single top-level array; for multi-page results, potentially multiple arrays
/// concatenated. Handles both shapes.
fn parse_issue_pages(text: &str) -> Option<Vec<serde_json::Value>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(Vec::new());
    }

    if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(arr);
    }

    let mut out = Vec::new();
    let de = serde_json::Deserializer::from_str(trimmed).into_iter::<serde_json::Value>();
    for next in de {
        match next {
            Ok(serde_json::Value::Array(arr)) => out.extend(arr),
            Ok(other) => out.push(other),
            Err(_) => return None,
        }
    }
    Some(out)
}

struct Comment {
    author: String,
    date: String,
    body: String,
}

struct IssueData {
    title: String,
    body: String,
    state: String,
    labels: Vec<String>,
    comments: Vec<Comment>,
    milestone: Option<String>,
}

impl IssueData {
    /// Return raw facts from GitHub — the consuming agent interprets them.
    fn to_facts(&self) -> HashMap<String, String> {
        let mut facts = HashMap::new();
        facts.insert("state".to_string(), self.state.clone());
        if !self.labels.is_empty() {
            facts.insert("labels".to_string(), self.labels.join(", "));
        }
        if let Some(ms) = &self.milestone {
            facts.insert("milestone".to_string(), ms.clone());
        }
        facts
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
        let mut body_parts = Vec::new();
        if !issue.body.is_empty() {
            body_parts.push(localize_markdown_images(&issue.body));
        }

        // Append comments so the consuming agent has full context
        if !issue.comments.is_empty() {
            body_parts.push("\n### Comments\n".to_string());
            for c in &issue.comments {
                body_parts.push(format!("**{}** ({}) — {}\n", c.date, c.author, c.body));
            }
        }

        let body = if body_parts.is_empty() {
            None
        } else {
            Some(body_parts.join("\n"))
        };

        Ok(ResourceFields {
            name: Some(issue.title.clone()),
            description: Some(issue.title.clone()),
            body,
            facts: issue.to_facts(),
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
                            body: if issue.body.is_empty() {
                                None
                            } else {
                                Some(issue.body.clone())
                            },
                            facts: issue.to_facts(),
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
