use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub enabled: bool,
    pub issuer_url: String,
    pub expected_audience: Option<String>,
    pub dev_mode: bool,
    /// Optional explicit userinfo endpoint URL. If `None`, the URL is derived
    /// from the OIDC discovery document's `userinfo_endpoint` or `{issuer}/userinfo`.
    pub userinfo_url: Option<String>,
    /// Trusted-issuer allowlist for pep's per-token issuer fallback
    /// (issue 895cbcea — the CPO IdP cut-over re-arm, gate-1 duty per
    /// issuer-law v2). `None`/empty = the ResourceServerClient is
    /// constructed WITHOUT the knob = byte-identical strict single-issuer
    /// validation (fail-closed preserved, the pep law). The configured
    /// `issuer_url` is always trusted and never needs listing.
    pub trusted_issuers: Option<Vec<String>>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            issuer_url: "http://localhost:8080".to_string(),
            expected_audience: None,
            dev_mode: false,
            userinfo_url: None,
            trusted_issuers: None,
        }
    }
}

impl AuthConfig {
    /// Parse the AUTH_TRUSTED_ISSUERS env CSV (issue 895cbcea): comma-
    /// separated issuer origins, each trimmed, empties dropped. Unset or
    /// all-empty → `None` (byte-identical construction, the pep law).
    pub fn parse_trusted_issuers(raw: Option<String>) -> Option<Vec<String>> {
        let parsed: Vec<String> = raw
            .map(|csv| {
                csv.split(',')
                    .map(|s| s.trim().trim_end_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        if parsed.is_empty() {
            None
        } else {
            Some(parsed)
        }
    }
}
