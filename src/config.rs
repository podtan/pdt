//! Configuration module for PDT

use crate::auth::config::AuthConfig;
use anyhow::{Context, Result};
use std::env;

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

/// Database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub username: String,
    pub password: String,
    pub database: String,
    pub tls: bool,
    pub tls_allow_invalid: bool,
}

impl DatabaseConfig {
    /// Build MongoDB connection string with authentication and TLS
    pub fn connection_string(&self) -> String {
        let mut conn = format!("{}/?authSource=admin", self.url.trim_end_matches('/'));

        if self.tls {
            conn.push_str("&tls=true");
            if self.tls_allow_invalid {
                conn.push_str("&tlsAllowInvalidCertificates=true");
            }
        }

        conn
    }
}

/// Cedar authorization configuration
#[derive(Debug, Clone)]
pub struct CedarConfig {
    pub enabled: bool,
    pub policy_path: String,
    pub schema_path: String,
    pub validate_on_load: bool,
    pub default_decision: String,
    /// Optional URL of a remote policy store endpoint.
    /// When set, policies are fetched from this URL at startup.
    pub policy_store_url: Option<String>,
    /// Optional Bearer token for the policy store endpoint.
    pub policy_store_token: Option<String>,
}

impl Default for CedarConfig {
    fn default() -> Self {
        Self {
            enabled: env::var("CEDAR_ENABLED")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            policy_path: env::var("CEDAR_POLICY_PATH")
                .unwrap_or_else(|_| "./policies".to_string()),
            schema_path: env::var("CEDAR_SCHEMA_PATH")
                .unwrap_or_else(|_| "./policies/schema.cedarschema".to_string()),
            validate_on_load: env::var("CEDAR_VALIDATE_ON_LOAD")
                .unwrap_or_else(|_| "true".to_string())
                .parse()
                .unwrap_or(true),
            default_decision: env::var("CEDAR_DEFAULT_DECISION")
                .unwrap_or_else(|_| "deny".to_string()),
            policy_store_url: env::var("CEDAR_POLICY_STORE_URL").ok(),
            policy_store_token: env::var("CEDAR_POLICY_STORE_TOKEN").ok(),
        }
    }
}

/// Convert PDT's CedarConfig to PEP's CedarConfig, injecting embedded policies.
impl From<CedarConfig> for pep::cedar::CedarConfig {
    fn from(config: CedarConfig) -> Self {
        use std::path::PathBuf;
        Self {
            policy_path: PathBuf::from(&config.policy_path),
            schema_path: Some(PathBuf::from(&config.schema_path)),
            entities_path: None,
            default_decision: match config.default_decision.as_str() {
                "allow" => pep::cedar::config::DefaultDecision::Allow,
                _ => pep::cedar::config::DefaultDecision::Deny,
            },
            validate_on_load: config.validate_on_load,
            policy_store_url: config.policy_store_url,
            policy_store_token: config.policy_store_token,
            embedded_policy: Some(include_str!("../policies/rbac.cedar")),
            embedded_schema: Some(include_str!("../policies/schema.cedarschema")),
        }
    }
}

/// Database backend type
#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseBackend {
    Mongodb,
    #[cfg(feature = "sqlite-backend")]
    Sqlite,
}

impl Default for DatabaseBackend {
    fn default() -> Self {
        DatabaseBackend::Mongodb
    }
}

/// Main application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub database_backend: DatabaseBackend,
    #[cfg(feature = "sqlite-backend")]
    pub sqlite_path: String,
    pub auth: AuthConfig,
    pub cedar: CedarConfig,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let cedar = CedarConfig::default();

        let database_backend = match env::var("PDT_DB_BACKEND")
            .unwrap_or_else(|_| "mongodb".to_string())
            .to_lowercase()
            .as_str()
        {
            #[cfg(feature = "sqlite-backend")]
            "sqlite" => DatabaseBackend::Sqlite,
            _ => DatabaseBackend::Mongodb,
        };

        Ok(Config {
            server: ServerConfig {
                host: env::var("PDT_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("PDT_PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()
                    .context("Invalid PDT_PORT")?,
            },
            database: DatabaseConfig {
                url: env::var("DOCUMENTDB_URL").unwrap_or_default(),
                username: env::var("DOCUMENTDB_USERNAME").unwrap_or_default(),
                password: env::var("DOCUMENTDB_PASSWORD").unwrap_or_default(),
                database: env::var("DOCUMENTDB_DATABASE").unwrap_or_default(),
                tls: env::var("DOCUMENTDB_TLS")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                tls_allow_invalid: env::var("DOCUMENTDB_TLS_ALLOW_INVALID")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
            database_backend,
            #[cfg(feature = "sqlite-backend")]
            sqlite_path: env::var("SQLITE_PATH")
                .unwrap_or_else(|_| "pdt.db".to_string()),
            auth: AuthConfig {
                enabled: env::var("AUTH_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                issuer_url: env::var("AUTH_ISSUER_URL")
                    .unwrap_or_else(|_| "http://localhost:8080".to_string()),
                expected_audience: env::var("AUTH_AUDIENCE").ok(),
                dev_mode: env::var("AUTH_DEV_MODE")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                userinfo_url: env::var("AUTH_USERINFO_URL").ok(),
            },
            cedar,
        })
    }
}
