use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::process::{Command, Stdio};

use crate::adapter::{Adapter, ResourceFields};
use crate::status::ManualStatus;

/// External adapter — runs a subprocess that speaks the amos adapter protocol.
///
/// Protocol:
///   <command> auth           → Browser login flow (interactive)
///   <command> auth-status    → JSON: {"authenticated": true/false}
///   <command> resolve <ref>  → JSON: {name, description, status, body}
///   <command> batch <json>   → JSON object keyed by reference
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

    fn build_command(&self) -> Result<(String, Vec<String>)> {
        let parts: Vec<&str> = self.command.split_whitespace().collect();
        if parts.is_empty() {
            bail!("empty command for adapter '{}'", self.uri_scheme);
        }
        let program = parts[0].to_string();
        let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();
        Ok((program, args))
    }

    fn run_command(&self, args: &[&str]) -> Result<Vec<u8>> {
        let (program, base_args) = self.build_command()?;

        let mut cmd = Command::new(&program);
        for arg in &base_args {
            cmd.arg(arg);
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

    /// Check if the adapter is authenticated.
    pub fn is_authenticated(&self) -> bool {
        let result = self.run_command(&["auth-status"]);
        match result {
            Ok(stdout) => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&stdout) {
                    json["authenticated"].as_bool().unwrap_or(false)
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    /// Run the interactive auth flow. Inherits stdin/stdout/stderr
    /// so the adapter can open a browser and interact with the user.
    pub fn authenticate(&self) -> Result<()> {
        let (program, base_args) = self.build_command()?;

        let mut cmd = Command::new(&program);
        for arg in &base_args {
            cmd.arg(arg);
        }
        cmd.arg("auth");

        // Inherit stdio so the adapter can interact with the user
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        let status = cmd.status().with_context(|| {
            format!("failed to run auth for adapter '{}'", self.uri_scheme)
        })?;

        if !status.success() {
            bail!("auth failed for adapter '{}'", self.uri_scheme);
        }

        Ok(())
    }

    /// Ensure the adapter is authenticated, running auth flow if needed.
    pub fn ensure_authenticated(&self) -> Result<()> {
        if !self.is_authenticated() {
            eprintln!(
                "amos: adapter '{}' requires authentication",
                self.uri_scheme
            );
            self.authenticate()?;

            if !self.is_authenticated() {
                bail!(
                    "adapter '{}' still not authenticated after auth flow",
                    self.uri_scheme
                );
            }
        }
        Ok(())
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
        // Check auth before resolving
        self.ensure_authenticated()?;

        let stdout = self.run_command(&["resolve", reference])?;
        let json: serde_json::Value =
            serde_json::from_slice(&stdout).context("parsing adapter JSON output")?;
        Ok(json_to_fields(&json))
    }

    fn resolve_batch(&self, references: &[&str]) -> Result<HashMap<String, ResourceFields>> {
        self.ensure_authenticated()?;

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
