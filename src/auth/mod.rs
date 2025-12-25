pub mod config;
pub mod error;
pub mod extractor;
pub mod middleware;

pub use config::AuthConfig;
pub use error::AuthError;
pub use extractor::AuthenticatedUser;
pub use middleware::AuthLayer;

#[cfg(test)]
mod tests;