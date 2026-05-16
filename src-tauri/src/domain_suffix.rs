//! DNS zone validation for macOS split-DNS (`/etc/resolver/<zone>`).
//!
//! A **single-label** zone like `dev` or `com` hijacks the entire TLD. A **multi-label**
//! zone like `myapp.com` only hijacks `*.myapp.com`; other `.com` names use normal DNS.

/// Single-label zones that must not be used as `/etc/resolver/<name>` on macOS.
const BLOCKED_SINGLE_LABEL_ZONES: &[&str] = &[
    "app", "dev", "home", "local", "internal", "ai", "co", "com", "de", "edu", "fr", "gov", "info",
    "int", "io", "me", "mil", "net", "org", "uk", "us", "arpa", "onion",
];

pub const RECOMMENDED_ZONES: &[&str] = &["test", "myapp.test", "myapp.com"];

pub fn normalize_dns_zone(raw: &str) -> Result<String, String> {
    let s = raw.trim().trim_start_matches('.').to_lowercase();
    if s.is_empty() {
        return Err("DNS zone cannot be empty".to_string());
    }
    if s.len() > 253 {
        return Err("DNS zone is too long".to_string());
    }
    let labels: Vec<&str> = s.split('.').filter(|l| !l.is_empty()).collect();
    if labels.is_empty() {
        return Err("DNS zone must contain at least one label".to_string());
    }
    for label in &labels {
        if label.len() > 63 {
            return Err("each label in the DNS zone must be at most 63 characters".to_string());
        }
        let ok = label
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !ok || label.starts_with('-') || label.ends_with('-') {
            return Err(
                "DNS zone labels must match [a-z0-9-] and not start/end with '-'".to_string(),
            );
        }
    }
    Ok(s)
}

pub fn validate_dns_zone(zone: &str) -> Result<(), String> {
    let z = normalize_dns_zone(zone)?;

    // macOS resolver file name = zone; only single-label public suffixes break whole TLDs.
    if !z.contains('.') && BLOCKED_SINGLE_LABEL_ZONES.contains(&z.as_str()) {
        let recommended = RECOMMENDED_ZONES
            .iter()
            .map(|x| format!("`{x}`"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "`.{z}` routes all `*.{z}` DNS through Domainest and breaks real sites on that TLD. \
             Use a private zone like `test`, or a subdomain you control (e.g. `myapp.com` for \
             `*.myapp.com` only — other `.com` sites stay on normal DNS). Examples: {recommended}."
        ));
    }

    Ok(())
}

/// True if `host` is `zone` or a subdomain of `zone` (e.g. `api.myapp.com` under `myapp.com`).
pub fn domain_under_zone(host: &str, zone: &str) -> bool {
    let zone = zone.trim_start_matches('.').to_lowercase();
    let host = host.trim_end_matches('.').to_lowercase();
    host == zone || host.ends_with(&format!(".{zone}"))
}

/// Whether macOS split-DNS for `zone` alone is enough to resolve `host`.
pub fn host_covered_by_zone_resolver(host: &str, zone: &str) -> bool {
    domain_under_zone(host, zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_single_label_dev_and_com() {
        assert!(validate_dns_zone("dev").is_err());
        assert!(validate_dns_zone("com").is_err());
    }

    #[test]
    fn allows_test_and_subdomain_zones() {
        assert!(validate_dns_zone("test").is_ok());
        assert!(validate_dns_zone("myapp").is_ok());
        assert!(validate_dns_zone("myapp.com").is_ok());
        assert!(validate_dns_zone("api.myapp.dev").is_ok());
    }

    #[test]
    fn domain_under_zone_cases() {
        assert!(domain_under_zone("app.test", "test"));
        assert!(!domain_under_zone("be-brand.dev", "test"));
        assert!(domain_under_zone("be-brand.dev", "be-brand.dev"));
    }
}
