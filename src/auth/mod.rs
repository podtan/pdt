pub mod config;
pub mod error;
pub mod extractor;
pub mod middleware;

pub use config::AuthConfig;
pub use error::AuthError;
pub use extractor::{AuthenticatedUser, Enrichment};
pub use middleware::{classify_enrichment, AuthEnrichment, AuthLayer};

#[cfg(test)]
mod tests;
