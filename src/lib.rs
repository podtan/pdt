//! Platform Data Toolkit (PDT) - Enterprise Knowledge Silo
//!
//! PDT is a centralized API server that serves as a knowledge silo for
//! company-specific concepts, documents, and their relationships.

pub mod auth;
pub mod cedar;
pub mod config;
pub mod db;
pub mod error;
pub mod handlers;
pub mod models;
pub mod openapi;
pub mod repository;
pub mod service;
pub mod tenant;

pub use config::Config;
pub use error::{ApiError, Result};
