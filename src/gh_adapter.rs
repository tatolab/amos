use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::adapter::{
    Adapter, AdapterNode, CreatedIssue, IssueSpec, MilestoneInfo, RelationshipKind, ResourceFields,
};
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

    /// Construct a `GhAdapter` whose default repo is inferred from the scan
    /// root's git remote. Falls back to `None` if the directory isn't a git
    /// checkout or the remote doesn't point at github.com.
    pub fn with_detected_repo(scan_root: &Path) -> Self {
        GhAdapter {
            default_repo: detect_github_repo(scan_root),
        }
    }

    /// Read the effective default repo (from the arg passed to `new()` or
    /// auto-detected from the scan root).
    pub fn default_repo(&self) -> Option<&str> {
        self.default_repo.as_deref()
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

    fn list_milestones(&self) -> Result<Vec<MilestoneInfo>> {
        let Some(repo) = self.default_repo.as_deref() else {
            return Ok(Vec::new());
        };
        fetch_milestones_graphql(repo)
    }

    fn list_nodes_in_milestone(&self, milestone: &str) -> Result<Vec<AdapterNode>> {
        let Some(repo) = self.default_repo.as_deref() else {
            return Ok(Vec::new());
        };
        fetch_milestone_issues_graphql(repo, milestone)
    }

    fn add_relationship(
        &self,
        from: &str,
        to: &str,
        kind: RelationshipKind,
    ) -> Result<()> {
        let (from_repo, from_num) = self.parse_ref(from)?;
        let (to_repo, to_num) = self.parse_ref(to)?;
        if from_repo != to_repo {
            bail!(
                "relationships across different repos aren't supported: {} → {}",
                from,
                to
            );
        }
        let Some(repo) = from_repo.as_deref().or(self.default_repo.as_deref()) else {
            bail!("no default repo — pass --repo or run inside a git checkout");
        };
        add_relationship_graphql(repo, from_num, to_num, kind)
    }

    fn create_issue(&self, spec: &IssueSpec) -> Result<CreatedIssue> {
        let Some(repo) = self.default_repo.as_deref() else {
            bail!("no default repo — run inside a git checkout or configure one");
        };
        create_issue_via_gh(repo, spec)
    }
}

// ---------------------------------------------------------------------------
// GraphQL helpers
//
// The GhAdapter talks to the REST API for single-issue view/comment and the
// paginated issue list, but the relationship fields (`blockedBy`, `blocking`,
// `parent`, `subIssues`) and the mutation endpoints for native relationships
// live in GraphQL only. The helpers below are intentionally stateless — they
// shell out to `gh api graphql`, the same auth path the REST calls use.
// ---------------------------------------------------------------------------

/// Detect a GitHub repo (`owner/name`) from the directory's git remote.
/// Returns `None` if the directory isn't a git checkout, has no remote, or
/// the remote doesn't point at github.com.
fn detect_github_repo(dir: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", dir.to_str()?, "remote", "get-url", "origin"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    parse_github_remote(&url)
}

/// Extract `owner/name` from typical github.com remote URL shapes:
/// - `https://github.com/owner/name.git`
/// - `git@github.com:owner/name.git`
/// - `ssh://git@github.com/owner/name.git`
fn parse_github_remote(url: &str) -> Option<String> {
    let stripped = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let without_suffix = stripped.strip_suffix(".git").unwrap_or(stripped);
    let parts: Vec<&str> = without_suffix.splitn(3, '/').collect();
    if parts.len() < 2 || parts[0].is_empty() || parts[1].is_empty() {
        return None;
    }
    Some(format!("{}/{}", parts[0], parts[1]))
}

/// Run a `gh api graphql` query and return the parsed JSON response. Query
/// variables are passed as `-F key=value`.
fn run_graphql(query: &str, variables: &[(&str, &str)]) -> Result<serde_json::Value> {
    let mut cmd = Command::new("gh");
    cmd.args(["api", "graphql", "-f", &format!("query={}", query)]);
    for (k, v) in variables {
        cmd.args(["-F", &format!("{}={}", k, v)]);
    }
    let output = cmd
        .output()
        .context("failed to run 'gh api graphql' — is gh CLI installed?")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh api graphql failed: {}", stderr.trim());
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("parsing graphql JSON response")?;
    if let Some(errors) = parsed.get("errors") {
        bail!("graphql errors: {}", errors);
    }
    Ok(parsed)
}

fn fetch_milestones_graphql(repo: &str) -> Result<Vec<MilestoneInfo>> {
    let (owner, name) = split_repo(repo)?;
    let query = r#"
        query($owner: String!, $name: String!) {
          repository(owner: $owner, name: $name) {
            milestones(first: 100, orderBy: {field: NUMBER, direction: ASC}) {
              nodes {
                title
                state
                openIssues: issues(states: OPEN) { totalCount }
                closedIssues: issues(states: CLOSED) { totalCount }
              }
            }
          }
        }
    "#;
    let response = run_graphql(query, &[("owner", owner), ("name", name)])?;
    let nodes = response
        .pointer("/data/repository/milestones/nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        let title = node
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if title.is_empty() {
            continue;
        }
        let state = node
            .get("state")
            .and_then(|v| v.as_str())
            .unwrap_or("OPEN")
            .to_string();
        let open_count = node
            .pointer("/openIssues/totalCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let closed_count = node
            .pointer("/closedIssues/totalCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        out.push(MilestoneInfo {
            title,
            state,
            open_count,
            closed_count,
        });
    }
    Ok(out)
}

fn fetch_milestone_issues_graphql(repo: &str, milestone_title: &str) -> Result<Vec<AdapterNode>> {
    let (owner, name) = split_repo(repo)?;
    // Pagination: we ask for up to 100 issues per page and re-query until
    // `hasNextPage` is false.
    let query = r#"
        query($owner: String!, $name: String!, $milestone: String!, $cursor: String) {
          repository(owner: $owner, name: $name) {
            milestones(query: $milestone, first: 10) {
              nodes {
                title
                issues(first: 100, after: $cursor, states: [OPEN, CLOSED]) {
                  pageInfo { hasNextPage endCursor }
                  nodes {
                    number title state
                    labels(first: 20) { nodes { name } }
                    milestone { title }
                    blockedBy(first: 50) { nodes { number } }
                    blocking(first: 50) { nodes { number } }
                    parent { number }
                    subIssues(first: 50) { nodes { number } }
                  }
                }
              }
            }
          }
        }
    "#;

    let mut cursor: Option<String> = None;
    let mut out: Vec<AdapterNode> = Vec::new();
    loop {
        let cursor_arg = cursor.as_deref().unwrap_or("");
        let mut vars: Vec<(&str, &str)> = vec![
            ("owner", owner),
            ("name", name),
            ("milestone", milestone_title),
        ];
        if !cursor_arg.is_empty() {
            vars.push(("cursor", cursor_arg));
        }
        let response = run_graphql(query, &vars)?;
        // The `milestones(query:)` search can return multiple partial-title
        // matches; filter to exact title only.
        let milestones = response
            .pointer("/data/repository/milestones/nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let Some(milestone) = milestones
            .iter()
            .find(|m| m.get("title").and_then(|v| v.as_str()) == Some(milestone_title))
        else {
            break;
        };
        let issues_obj = milestone
            .pointer("/issues")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let issue_nodes = issues_obj
            .pointer("/nodes")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for issue in issue_nodes {
            out.push(issue_json_to_adapter_node(&issue, repo));
        }
        let has_next = issues_obj
            .pointer("/pageInfo/hasNextPage")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !has_next {
            break;
        }
        cursor = issues_obj
            .pointer("/pageInfo/endCursor")
            .and_then(|v| v.as_str())
            .map(String::from);
        if cursor.is_none() {
            break;
        }
    }

    Ok(out)
}

fn issue_json_to_adapter_node(issue: &serde_json::Value, repo: &str) -> AdapterNode {
    let number = issue.get("number").and_then(|v| v.as_u64()).unwrap_or(0);
    let title = issue
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let state = issue
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("OPEN")
        .to_string();
    let labels: Vec<String> = issue
        .pointer("/labels/nodes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|l| l.get("name").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let milestone = issue
        .pointer("/milestone/title")
        .and_then(|v| v.as_str())
        .map(String::from);

    let extract_numbers = |path: &str| -> Vec<String> {
        issue
            .pointer(path)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|n| n.get("number").and_then(|v| v.as_u64()))
                    .map(|n| format!("@github:{}#{}", repo, n))
                    .collect()
            })
            .unwrap_or_default()
    };

    let blocked_by = extract_numbers("/blockedBy/nodes");
    let blocks = extract_numbers("/blocking/nodes");
    let sub_issues = extract_numbers("/subIssues/nodes");
    let parent = issue
        .pointer("/parent/number")
        .and_then(|v| v.as_u64())
        .map(|n| format!("@github:{}#{}", repo, n));

    let mut facts: HashMap<String, String> = HashMap::new();
    facts.insert("state".to_string(), state);
    if !labels.is_empty() {
        facts.insert("labels".to_string(), labels.join(", "));
    }
    if let Some(ms) = milestone {
        facts.insert("milestone".to_string(), ms);
    }

    AdapterNode {
        name: format!("@github:{}#{}", repo, number),
        title,
        facts,
        blocked_by,
        blocks,
        parent,
        sub_issues,
    }
}

/// Look up an issue's GraphQL node ID. Required for the relationship
/// mutations — they take opaque IDs, not numbers.
fn fetch_issue_node_id(repo: &str, number: u64) -> Result<String> {
    let (owner, name) = split_repo(repo)?;
    let query = r#"
        query($owner: String!, $name: String!, $number: Int!) {
          repository(owner: $owner, name: $name) {
            issue(number: $number) { id }
          }
        }
    "#;
    let num_str = number.to_string();
    let vars = [("owner", owner), ("name", name), ("number", num_str.as_str())];
    // `-F` stringifies numbers, which is what GraphQL's Int scalar accepts
    // over the CLI. (The server coerces.)
    let response = run_graphql(query, &vars)?;
    response
        .pointer("/data/repository/issue/id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("issue {}#{} not found", repo, number))
}

fn add_relationship_graphql(
    repo: &str,
    from_num: u64,
    to_num: u64,
    kind: RelationshipKind,
) -> Result<()> {
    let from_id = fetch_issue_node_id(repo, from_num)?;
    let to_id = fetch_issue_node_id(repo, to_num)?;

    match kind {
        RelationshipKind::BlockedBy => {
            // from is blocked by to — addBlockedBy takes issueId + blockingIssueId
            let query = r#"
                mutation($issue: ID!, $blocking: ID!) {
                  addBlockedBy(input: {issueId: $issue, blockingIssueId: $blocking}) {
                    issue { number }
                  }
                }
            "#;
            run_graphql(query, &[("issue", &from_id), ("blocking", &to_id)])?;
        }
        RelationshipKind::Blocks => {
            // from blocks to — same mutation, flipped args
            let query = r#"
                mutation($issue: ID!, $blocking: ID!) {
                  addBlockedBy(input: {issueId: $issue, blockingIssueId: $blocking}) {
                    issue { number }
                  }
                }
            "#;
            run_graphql(query, &[("issue", &to_id), ("blocking", &from_id)])?;
        }
        RelationshipKind::SubIssueOf => {
            // from is a sub-issue of to — addSubIssue takes parent (to) +
            // child (from) as sub-issue.
            let query = r#"
                mutation($parent: ID!, $child: ID!) {
                  addSubIssue(input: {issueId: $parent, subIssueId: $child}) {
                    issue { number }
                  }
                }
            "#;
            run_graphql(query, &[("parent", &to_id), ("child", &from_id)])?;
        }
    }
    Ok(())
}

fn split_repo(repo: &str) -> Result<(&str, &str)> {
    repo.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("malformed repo '{}', expected owner/name", repo))
}

/// Create a new GitHub issue via `gh issue create`. Applies title + body
/// + milestone + labels in the same call; relationships + issue type are
/// separate mutations the caller runs afterward.
fn create_issue_via_gh(repo: &str, spec: &IssueSpec) -> Result<CreatedIssue> {
    if spec.title.trim().is_empty() {
        bail!("issue title is empty");
    }
    let mut cmd = Command::new("gh");
    cmd.args(["issue", "create", "--repo", repo]);
    cmd.args(["--title", &spec.title]);
    cmd.args(["--body", &spec.body]);
    if let Some(ms) = &spec.milestone {
        cmd.args(["--milestone", ms]);
    }
    for label in &spec.labels {
        cmd.args(["--label", label]);
    }

    let output = cmd.output().context("failed to spawn 'gh issue create'")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh issue create failed: {}", stderr.trim());
    }
    // gh prints the issue URL to stdout on success.
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let number = url
        .rsplit_once('/')
        .and_then(|(_, n)| n.parse::<u64>().ok())
        .ok_or_else(|| anyhow::anyhow!("couldn't parse issue number from gh output: {}", url))?;

    if let Some(type_name) = &spec.issue_type {
        set_issue_type_by_name(repo, number, type_name)?;
    }

    Ok(CreatedIssue {
        name: format!("@github:{}#{}", repo, number),
        number,
        url,
    })
}

/// Apply a repository-level issue type (e.g. "Bug", "Feature", "Task") to
/// an already-created issue. `gh issue create` doesn't support `--type`,
/// so we set it via the `updateIssueIssueType` mutation.
fn set_issue_type_by_name(repo: &str, number: u64, type_name: &str) -> Result<()> {
    let issue_id = fetch_issue_node_id(repo, number)?;
    let types = fetch_issue_types_graphql(repo)?;
    let type_id = types
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(type_name))
        .map(|(_, id)| id.clone())
        .ok_or_else(|| {
            let available: Vec<&str> = types.iter().map(|(n, _)| n.as_str()).collect();
            anyhow::anyhow!(
                "issue type '{}' not configured on {} (available: {})",
                type_name,
                repo,
                available.join(", ")
            )
        })?;
    let query = r#"
        mutation($issue: ID!, $type: ID!) {
          updateIssueIssueType(input: {issueId: $issue, issueTypeId: $type}) {
            issue { number }
          }
        }
    "#;
    run_graphql(query, &[("issue", &issue_id), ("type", &type_id)])?;
    Ok(())
}

/// Fetch the repository's configured issue types. Returns [(name, id)]
/// pairs. Repo admins manage the list — amos doesn't create types itself.
fn fetch_issue_types_graphql(repo: &str) -> Result<Vec<(String, String)>> {
    let (owner, name) = split_repo(repo)?;
    let query = r#"
        query($owner: String!, $name: String!) {
          repository(owner: $owner, name: $name) {
            issueTypes(first: 50) { nodes { id name } }
          }
        }
    "#;
    let response = run_graphql(query, &[("owner", owner), ("name", name)])?;
    let nodes = response
        .pointer("/data/repository/issueTypes/nodes")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        let name = node
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let id = node
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !name.is_empty() && !id.is_empty() {
            out.push((name, id));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_remote() {
        assert_eq!(
            parse_github_remote("https://github.com/tatolab/amos.git"),
            Some("tatolab/amos".to_string())
        );
        assert_eq!(
            parse_github_remote("https://github.com/tatolab/amos"),
            Some("tatolab/amos".to_string())
        );
    }

    #[test]
    fn parse_ssh_remote() {
        assert_eq!(
            parse_github_remote("git@github.com:tatolab/amos.git"),
            Some("tatolab/amos".to_string())
        );
        assert_eq!(
            parse_github_remote("ssh://git@github.com/tatolab/amos.git"),
            Some("tatolab/amos".to_string())
        );
    }

    #[test]
    fn parse_non_github_remote_returns_none() {
        assert_eq!(parse_github_remote("https://gitlab.com/foo/bar.git"), None);
        assert_eq!(parse_github_remote("https://example.com/foo/bar.git"), None);
    }

    #[test]
    fn parse_malformed_remote_returns_none() {
        assert_eq!(parse_github_remote("not-a-url"), None);
        assert_eq!(parse_github_remote("https://github.com/"), None);
        assert_eq!(parse_github_remote("https://github.com/just-owner"), None);
    }

    #[test]
    fn split_repo_handles_owner_name() {
        let (owner, name) = split_repo("tatolab/amos").unwrap();
        assert_eq!(owner, "tatolab");
        assert_eq!(name, "amos");
    }

    #[test]
    fn issue_json_to_adapter_node_extracts_relationships() {
        let issue = serde_json::json!({
            "number": 42,
            "title": "Test issue",
            "state": "OPEN",
            "labels": { "nodes": [{"name": "bug"}, {"name": "linux"}] },
            "milestone": { "title": "Some Milestone" },
            "blockedBy": { "nodes": [{"number": 10}, {"number": 11}] },
            "blocking": { "nodes": [{"number": 99}] },
            "parent": { "number": 5 },
            "subIssues": { "nodes": [{"number": 100}] }
        });
        let node = issue_json_to_adapter_node(&issue, "tatolab/amos");
        assert_eq!(node.name, "@github:tatolab/amos#42");
        assert_eq!(node.title, "Test issue");
        assert_eq!(
            node.blocked_by,
            vec!["@github:tatolab/amos#10", "@github:tatolab/amos#11"]
        );
        assert_eq!(node.blocks, vec!["@github:tatolab/amos#99"]);
        assert_eq!(node.parent.as_deref(), Some("@github:tatolab/amos#5"));
        assert_eq!(node.sub_issues, vec!["@github:tatolab/amos#100"]);
        assert_eq!(node.facts.get("state").map(|s| s.as_str()), Some("OPEN"));
        assert_eq!(node.facts.get("labels").map(|s| s.as_str()), Some("bug, linux"));
        assert_eq!(
            node.facts.get("milestone").map(|s| s.as_str()),
            Some("Some Milestone")
        );
    }

    #[test]
    fn create_issue_rejects_empty_title() {
        let spec = IssueSpec {
            title: "   ".to_string(),
            body: "Some body".to_string(),
            ..Default::default()
        };
        let err = create_issue_via_gh("tatolab/amos", &spec).unwrap_err();
        assert!(format!("{}", err).contains("title is empty"));
    }

    #[test]
    fn issue_json_to_adapter_node_handles_empty_relationships() {
        let issue = serde_json::json!({
            "number": 1,
            "title": "",
            "state": "CLOSED"
        });
        let node = issue_json_to_adapter_node(&issue, "tatolab/amos");
        assert!(node.blocked_by.is_empty());
        assert!(node.blocks.is_empty());
        assert!(node.sub_issues.is_empty());
        assert!(node.parent.is_none());
        assert_eq!(node.facts.get("state").map(|s| s.as_str()), Some("CLOSED"));
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
