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

/// Main application configuration
#[derive(Debug, Clone)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub auth: AuthConfig,
}

impl Config {
    /// Load configuration from environment variables
    pub fn from_env() -> Result<Self> {
        Ok(Config {
            server: ServerConfig {
                host: env::var("PDT_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("PDT_PORT")
                    .unwrap_or_else(|_| "8080".to_string())
                    .parse()
                    .context("Invalid PDT_PORT")?,
            },
            database: DatabaseConfig {
                url: env::var("DOCUMENTDB_URL").context("DOCUMENTDB_URL must be set")?,
                username: env::var("DOCUMENTDB_USERNAME")
                    .context("DOCUMENTDB_USERNAME must be set")?,
                password: env::var("DOCUMENTDB_PASSWORD")
                    .context("DOCUMENTDB_PASSWORD must be set")?,
                database: env::var("DOCUMENTDB_DATABASE")
                    .context("DOCUMENTDB_DATABASE must be set")?,
                tls: env::var("DOCUMENTDB_TLS")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                tls_allow_invalid: env::var("DOCUMENTDB_TLS_ALLOW_INVALID")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
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
            },
        })
    }
}
