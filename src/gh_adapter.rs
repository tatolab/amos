use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::adapter::{Adapter, ResourceFields};
use crate::status::ManualStatus;

/// GitHub adapter — resolves `gh:` URIs via the `gh` CLI.
///
/// URI formats:
/// - `gh:15` — issue #15 in the current repo
/// - `gh:owner/repo#15` — issue #15 in a specific repo
pub struct GhAdapter {
    /// Default repo (e.g. "tatolab/amos"). If None, uses current repo.
    default_repo: Option<String>,
}

impl GhAdapter {
    pub fn new(default_repo: Option<String>) -> Self {
        GhAdapter { default_repo }
    }

    /// Parse a reference into (repo, issue_number).
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

    /// Fetch a single issue via `gh issue view`.
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

    /// Batch fetch issues via `gh issue list`.
    fn fetch_issues_batch(
        &self,
        repo: Option<&str>,
        numbers: &[u64],
    ) -> Result<HashMap<u64, IssueData>> {
        if numbers.is_empty() {
            return Ok(HashMap::new());
        }

        // gh issue list doesn't filter by number directly in older versions,
        // so we fetch individually. For repos with many issues, a single
        // `gh api` call would be more efficient — optimize later.
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
    fn to_status(&self) -> Option<ManualStatus> {
        match self.state.as_str() {
            "CLOSED" => Some(ManualStatus::Done),
            "OPEN" => {
                if self.labels.iter().any(|l| l == "in-progress") {
                    Some(ManualStatus::InProgress)
                } else {
                    None // Open with no label = not started
                }
            }
            _ => None,
        }
    }
}

impl Adapter for GhAdapter {
    fn scheme(&self) -> &str {
        "gh"
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        let (repo, number) = self.parse_ref(reference)?;
        let issue = self.fetch_issue(repo.as_deref(), number)?;

        Ok(ResourceFields {
            name: Some(issue.title.clone()),
            description: Some(issue.title.clone()),
            status: issue.to_status(),
            body: if issue.body.is_empty() {
                None
            } else {
                Some(issue.body)
            },
        })
    }

    fn resolve_batch(&self, references: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        // Group by repo
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
