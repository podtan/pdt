//! Search service

use crate::error::Result;
use crate::models::Asset;
use crate::service::Services;
use std::collections::HashMap;

/// Service for search business logic
pub struct SearchService;

impl SearchService {
    /// Search assets by text and tags
    pub async fn search(
        services: &Services,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        services
            .assets()
            .search(query, tag_filters, limit, cursor)
            .await
    }

    /// Compact search: page rows carry NO content — the caller fetches
    /// bodies for the visible page via `get_contents` after the Cedar
    /// filter (issue eea5bcff).
    pub async fn search_compact(
        services: &Services,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        services
            .assets()
            .search_compact(query, tag_filters, limit, cursor)
            .await
    }

    /// Batch content fetch for the visible page (compact-path second phase).
    pub async fn get_contents(
        services: &Services,
        ids: &[String],
    ) -> Result<HashMap<String, String>> {
        services.assets().get_contents(ids).await
    }
}
