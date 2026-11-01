pub mod config;
pub mod error;
pub mod extractor;
pub mod middleware;

use pep::oidc_resource_server::ResourceServerClient;

pub use config::AuthConfig;
pub use error::AuthError;
pub use extractor::{AuthenticatedUser, Enrichment};
pub use middleware::{classify_enrichment, AuthEnrichment, AuthLayer};

/// Build the ResourceServerClient for the production auth path (issue
/// 895cbcea, the 0.3.23 trusted-issuers knob). Lives in the lib so the
/// gate tests exercise the EXACT production shape (the e0db0993 pattern).
///
/// Unset/empty `trusted_issuers` → the client is constructed WITHOUT the
/// knob = byte-identical strict single-issuer validation (fail-closed
/// preserved, the pep law). Non-empty → `with_trusted_issuers` arms the
/// per-token issuer fallback bounded by the allowlist.
pub fn build_auth_client(config: &AuthConfig) -> ResourceServerClient {
    let client = ResourceServerClient::new();
    match config.trusted_issuers.as_deref() {
        Some(list) if !list.is_empty() => client.with_trusted_issuers(list.to_vec()),
        _ => client,
    }
}

#[cfg(test)]
mod tests;
