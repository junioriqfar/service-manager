//! Read-only Windows Firewall snapshot for the Settings UI.
//!
//! This module never calls `New-NetFirewallRule`, `Set-NetFirewallRule`, or
//! `Remove-NetFirewallRule`. Operators must change firewall rules in Windows
//! Firewall; Doctor keeps checking port 22 only.

use std::env;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use sm_updater::runner::{Runner, SystemRunner};

const FIREWALL_QUERY_TIMEOUT: Duration = Duration::from_secs(20);

const FIREWALL_QUERY: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference="Stop";
$map = @{};
Get-NetFirewallRule -Direction Inbound -ErrorAction Stop | ForEach-Object {
  if ($_.DisplayName -match "OpenSSH|SSH" -or $_.Name -match "OpenSSH|SSH") {
    $map[$_.Name] = $_;
  }
};
$ErrorActionPreference="SilentlyContinue";
Get-NetFirewallPortFilter -ErrorAction SilentlyContinue |
  Where-Object { @($_.LocalPort) -contains "22" } |
  ForEach-Object {
    $rule = $_ | Get-NetFirewallRule -ErrorAction SilentlyContinue;
    if ($rule -and [string]$rule.Direction -eq "Inbound") {
      $map[$rule.Name] = $rule;
    }
  };
$ErrorActionPreference="Stop";
$map.Values | ForEach-Object {
  $pf = $_ | Get-NetFirewallPortFilter -ErrorAction SilentlyContinue;
  [pscustomobject]@{ name=$_.Name; displayName=$_.DisplayName; enabled=[string]$_.Enabled;
    direction=[string]$_.Direction; action=[string]$_.Action; profile=[string]$_.Profile;
    localPort=(@($pf.LocalPort) -join ",") }
} | ConvertTo-Json -Compress
"#;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallRuleView {
    pub name: String,
    pub display_name: String,
    pub enabled: String,
    pub direction: String,
    pub action: String,
    pub profile: String,
    pub local_port: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallStatusView {
    pub supported: bool,
    pub rules: Vec<FirewallRuleView>,
    pub message: String,
}

fn json_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.trim().to_string(),
        serde_json::Value::Bool(value) => {
            if *value {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(json_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(","),
        _ => String::new(),
    }
}

fn field(value: &serde_json::Value, name: &str) -> String {
    value
        .as_object()
        .and_then(|map| {
            map.iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
        .map(json_text)
        .unwrap_or_default()
}

fn rule_from_json(value: &serde_json::Value) -> Option<FirewallRuleView> {
    if !value.is_object() {
        return None;
    }
    Some(FirewallRuleView {
        name: field(value, "name"),
        display_name: field(value, "displayName"),
        enabled: field(value, "enabled"),
        direction: field(value, "direction"),
        action: field(value, "action"),
        profile: field(value, "profile"),
        local_port: field(value, "localPort"),
    })
}

/// Parse `ConvertTo-Json` output. Handles an array, one object (PowerShell
/// collapses single-element arrays), empty output, and invalid JSON.
pub fn parse_firewall_rules(raw: &str) -> Vec<FirewallRuleView> {
    let raw = raw.trim().trim_start_matches('\u{feff}');
    if raw.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    match value {
        serde_json::Value::Array(items) => items.iter().filter_map(rule_from_json).collect(),
        serde_json::Value::Object(_) => rule_from_json(&value).into_iter().collect(),
        _ => Vec::new(),
    }
}

fn powershell_exe() -> PathBuf {
    env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

fn query_firewall() -> Result<String, String> {
    let args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        FIREWALL_QUERY.to_string(),
    ];
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let output = SystemRunner
        .run(&powershell_exe(), &args, &cwd, FIREWALL_QUERY_TIMEOUT)
        .map_err(|error| format!("Failed to run PowerShell: {error}"))?;
    if !output.success() {
        let text = output.combined();
        return Err(if text.trim().is_empty() {
            format!("PowerShell exited with code {}.", output.exit_code)
        } else {
            sm_core::redact_secrets(&text)
        });
    }
    Ok(output.stdout)
}

/// Read inbound rules that look like SSH (name/display name match or port 22).
/// Failure is reported as `supported=false`; no rule is ever modified.
pub fn ssh_firewall_status() -> FirewallStatusView {
    match query_firewall() {
        Ok(raw) => {
            let rules = parse_firewall_rules(&raw);
            let message = if rules.is_empty() {
                "No inbound SSH firewall rules found. Rules may use a different display name, or SSH may not be enabled."
                    .to_string()
            } else {
                format!(
                    "{} inbound SSH firewall rule{} found.",
                    rules.len(),
                    if rules.len() == 1 { "" } else { "s" }
                )
            };
            FirewallStatusView {
                supported: true,
                rules,
                message,
            }
        }
        Err(error) => FirewallStatusView {
            supported: false,
            rules: Vec::new(),
            message: format!(
                "Windows Firewall rules could not be read. Change firewall rules manually in Windows Firewall. {error}"
            ),
        },
    }
}

/// Tauri command name is `ssh_firewall_status`; the Rust helper above keeps its
/// snapshot name for direct testing.
#[tauri::command(rename = "ssh_firewall_status")]
pub async fn read_ssh_firewall_status() -> Result<FirewallStatusView, String> {
    tauri::async_runtime::spawn_blocking(ssh_firewall_status)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_array_and_maps_fields() {
        let raw = r#"[
            {"name":"OpenSSH-Server-In-TCP","displayName":"OpenSSH SSH Server (sshd)","enabled":"True","direction":"Inbound","action":"Allow","profile":"Domain, Private","localPort":"22"},
            {"name":"Custom","displayName":"SSH custom","enabled":"False","direction":"Inbound","action":"Block","profile":"Public","localPort":"22,2222"}
        ]"#;
        let rules = parse_firewall_rules(raw);
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "OpenSSH-Server-In-TCP");
        assert_eq!(rules[0].display_name, "OpenSSH SSH Server (sshd)");
        assert_eq!(rules[0].enabled, "True");
        assert_eq!(rules[0].direction, "Inbound");
        assert_eq!(rules[0].action, "Allow");
        assert_eq!(rules[0].profile, "Domain, Private");
        assert_eq!(rules[0].local_port, "22");
        assert_eq!(rules[1].display_name, "SSH custom");
        assert_eq!(rules[1].enabled, "False");
        assert_eq!(rules[1].action, "Block");
        assert_eq!(rules[1].local_port, "22,2222");
    }

    #[test]
    fn parses_single_object_and_case_insensitive_keys() {
        let raw = r#"{"Name":"SSH","DisplayName":"SSH Server","Enabled":true,"Direction":"Inbound","Action":"Allow","Profile":"Private","LocalPort":22}"#;
        let rules = parse_firewall_rules(raw);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "SSH");
        assert_eq!(rules[0].display_name, "SSH Server");
        assert_eq!(rules[0].enabled, "True");
        assert_eq!(rules[0].local_port, "22");
    }

    #[test]
    fn empty_and_invalid_output_returns_empty_list() {
        assert!(parse_firewall_rules("").is_empty());
        assert!(parse_firewall_rules("   \r\n").is_empty());
        assert!(parse_firewall_rules("[]").is_empty());
        assert!(parse_firewall_rules("not-json").is_empty());
        assert!(parse_firewall_rules("null").is_empty());
        assert!(parse_firewall_rules("\"text\"").is_empty());
        assert!(parse_firewall_rules("[1, true, null]").is_empty());
    }

    #[test]
    fn serializes_camel_case_for_the_ui() {
        let rules = parse_firewall_rules(
            r#"[{"name":"SSH","displayName":"SSH Server","enabled":"True","direction":"Inbound","action":"Allow","profile":"Private","localPort":"22"}]"#,
        );
        let status = FirewallStatusView {
            supported: true,
            rules,
            message: "1 inbound SSH firewall rule found.".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"displayName\":\"SSH Server\""));
        assert!(json.contains("\"localPort\":\"22\""));
        assert!(json.contains("\"supported\":true"));
    }
}
