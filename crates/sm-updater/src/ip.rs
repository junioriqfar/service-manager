use std::env;
use std::net::IpAddr;

use ipnet::IpNet;

use crate::registry::{AppConfig, UpdaterConfig};

/// IP client dari environment SSH, sama seperti ProjectUpdate.ps1.
pub fn client_ip_from_env() -> Option<IpAddr> {
    let raw = env::var("SSH_CONNECTION")
        .ok()
        .or_else(|| env::var("SSH_CLIENT").ok())?;
    let first = raw.split_whitespace().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let normalized = first.strip_prefix("::ffff:").unwrap_or(first);
    normalized.parse().ok()
}

pub fn ip_matches(ip: IpAddr, pattern: &str) -> bool {
    let pattern = pattern.trim();
    if let Ok(network) = pattern.parse::<IpNet>() {
        return network.contains(&ip);
    }
    if let Ok(address) = pattern.parse::<IpAddr>() {
        return address == ip;
    }
    false
}

pub fn ip_allowed(ip: IpAddr, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| ip_matches(ip, pattern))
}

/// Cek dua lapis: allowlist global, lalu allowlist per app (jika diisi).
pub fn app_allowed_for_ip(
    config: &UpdaterConfig,
    app: &AppConfig,
    client_ip: Option<IpAddr>,
) -> bool {
    let patterns: &[String] = if app.allowed_client_ips.is_empty() {
        &config.trigger.allowed_client_ips
    } else {
        &app.allowed_client_ips
    };

    match client_ip {
        Some(ip) => patterns.is_empty() || ip_allowed(ip, patterns),
        None => config.trigger.allow_local,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_ip_and_cidr() {
        let ip: IpAddr = "10.255.240.143".parse().unwrap();
        assert!(ip_matches(ip, "10.255.240.143"));
        assert!(ip_matches(ip, "10.255.240.0/24"));
        assert!(!ip_matches(ip, "10.255.240.0/25"));
        assert!(!ip_matches(ip, "10.255.241.0/24"));
    }

    #[test]
    fn matches_ipv6() {
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(ip_matches(ip, "::1"));
        assert!(ip_matches(ip, "::1/128"));
        assert!(!ip_matches(ip, "2001:db8::/32"));
    }

    #[test]
    fn app_specific_allowlist_overrides_global() {
        let mut config = UpdaterConfig::default();
        config.trigger.allowed_client_ips = vec!["10.0.0.1".into()];
        let mut app = AppConfig {
            allowed_client_ips: vec!["10.255.240.0/24".into()],
            ..AppConfig::default()
        };
        let allowed: IpAddr = "10.255.240.143".parse().unwrap();
        assert!(app_allowed_for_ip(&config, &app, Some(allowed)));
        let denied: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!app_allowed_for_ip(&config, &app, Some(denied)));

        app.allowed_client_ips.clear();
        assert!(app_allowed_for_ip(&config, &app, Some(denied)));
    }

    #[test]
    fn local_trigger_uses_allow_local() {
        let mut config = UpdaterConfig::default();
        config.trigger.allow_local = false;
        let app = AppConfig::default();
        assert!(!app_allowed_for_ip(&config, &app, None));
        config.trigger.allow_local = true;
        assert!(app_allowed_for_ip(&config, &app, None));
    }
}
