use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::adapter::{Adapter, ResourceFields};
use crate::status::ManualStatus;

/// External adapter — runs a subprocess that speaks the amos adapter protocol.
///
/// Protocol:
///   <command> resolve <reference>  → JSON to stdout
///   <command> batch <json-array>   → JSON object keyed by reference
///
/// JSON shape for resolve:
/// {
///   "name": "optional string",
///   "description": "optional string",
///   "status": "done" | "in-progress" | null,
///   "body": "optional string"
/// }
///
/// Any executable that reads args and prints JSON can be an adapter.
/// Write them in Python, TypeScript, Go, shell — anything.
pub struct ExternalAdapter {
    uri_scheme: String,
    command: String,
}

impl ExternalAdapter {
    pub fn new(scheme: &str, command: &str) -> Self {
        ExternalAdapter {
            uri_scheme: scheme.to_string(),
            command: command.to_string(),
        }
    }

    fn run_command(&self, args: &[&str]) -> Result<Vec<u8>> {
        let parts: Vec<&str> = self.command.split_whitespace().collect();
        if parts.is_empty() {
            bail!("empty command for adapter '{}'", self.uri_scheme);
        }

        let mut cmd = Command::new(parts[0]);
        for &part in &parts[1..] {
            cmd.arg(part);
        }
        for &arg in args {
            cmd.arg(arg);
        }

        let output = cmd.output().with_context(|| {
            format!(
                "failed to run adapter '{}' (command: {})",
                self.uri_scheme, self.command
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "adapter '{}' failed: {}",
                self.uri_scheme,
                stderr.trim()
            );
        }

        Ok(output.stdout)
    }
}

fn parse_status(value: &serde_json::Value) -> Option<ManualStatus> {
    match value.as_str() {
        Some("done") => Some(ManualStatus::Done),
        Some("in-progress") => Some(ManualStatus::InProgress),
        _ => None,
    }
}

fn json_to_fields(json: &serde_json::Value) -> ResourceFields {
    ResourceFields {
        name: json["name"].as_str().map(String::from),
        description: json["description"].as_str().map(String::from),
        status: parse_status(&json["status"]),
        body: json["body"].as_str().map(String::from),
    }
}

impl Adapter for ExternalAdapter {
    fn scheme(&self) -> &str {
        &self.uri_scheme
    }

    fn resolve(&self, reference: &str) -> Result<ResourceFields> {
        let stdout = self.run_command(&["resolve", reference])?;
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).context("parsing adapter JSON output")?;
        Ok(json_to_fields(&json))
    }

    fn resolve_batch(&self, references: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        let refs_json = serde_json::to_string(references).context("serializing references")?;
        let stdout = self.run_command(&["batch", &refs_json])?;
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).context("parsing adapter batch JSON output")?;

        let mut results = HashMap::new();
        if let Some(obj) = json.as_object() {
            for (key, value) in obj {
                results.insert(key.clone(), json_to_fields(value));
            }
        }
        Ok(results)
    }
}
