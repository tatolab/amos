use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use crate::adapter::{Adapter, ResourceFields};

/// URL adapter — downloads remote content to a local cache.
///
/// URI format: `url:https://example.com/path/to/resource`
///
/// Images are downloaded and returned as local paths.
/// Text content is fetched and returned inline.
pub struct UrlAdapter;

impl UrlAdapter {
    pub fn new() -> Self {
        UrlAdapter
    }
}

impl Adapter for UrlAdapter {
    fn scheme(&self) -> &str {
        "url"
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        let filename = reference.rsplit('/').next().unwrap_or("file");

        if is_image_url(filename) {
            let local_path = download_to_cache(reference, filename)?;
            Ok(ResourceFields {
                name: None,
                description: None,
                facts: HashMap::new(),
                body: Some(format!("![{}]({})", filename, local_path.display())),
            })
        } else {
            // Text content — fetch and inline
            let content = fetch_text(reference)?;
            let ext = filename.rsplit('.').next().unwrap_or("");
            Ok(ResourceFields {
                name: None,
                description: None,
                facts: HashMap::new(),
                body: Some(if ext.is_empty() {
                    content
                } else {
                    format!("```{}\n{}\n```", ext, content)
                }),
            })
        }
    }
}

fn is_image_url(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

fn fetch_text(url: &str) -> Result<String> {
    let output = Command::new("curl")
        .args(["-sL", url])
        .output()
        .context("failed to run curl")?;

    if !output.status.success() {
        bail!("curl failed fetching {}", url);
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Download a URL to the amos cache directory, return the local path.
/// Shared by any adapter that needs to cache remote files.
pub fn download_to_cache(url: &str, filename: &str) -> Result<PathBuf> {
    let cache_dir = std::env::temp_dir().join("amos-cache");
    std::fs::create_dir_all(&cache_dir).context("creating amos cache dir")?;

    let hash = simple_hash(url);
    let ext = filename.rsplit('.').next().unwrap_or("");
    let cached_name = if ext.is_empty() {
        format!("{}", hash)
    } else {
        format!("{}.{}", hash, ext)
    };
    let local_path = cache_dir.join(&cached_name);

    if local_path.exists() {
        return Ok(local_path);
    }

    let output = Command::new("curl")
        .args(["-sL", "-o"])
        .arg(&local_path)
        .arg(url)
        .output()
        .context("failed to run curl")?;

    if !output.status.success() {
        bail!("curl failed downloading {}", url);
    }

    Ok(local_path)
}

fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}
