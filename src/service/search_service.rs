//! Search service

use crate::error::Result;
use crate::models::{Asset, SearchIndexDrift};
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

    /// Count assets matching the search filter — the true total for
    /// PaginatedResponse (issue b040d645: the field existed but was never
    /// populated; boards rendered page-local counts as if they were the
    /// whole set). Cursor-independent, pre-visibility.
    pub async fn count(
        services: &Services,
        query: Option<&str>,
        tag_filters: &[(String, String)],
    ) -> Result<u64> {
        services.assets().count_search(query, tag_filters).await
    }

    /// Search-index reconcile — the refresh contract for the derived FTS
    /// index (the list-shows/GET-404 family: e410455a, 92b7a94e,
    /// 49fe8037). Dry-run reports the drift; repair=true rebuilds the
    /// index from the assets table (the truth) and returns the
    /// post-repair state.
    pub async fn reconcile(
        services: &Services,
        repair: bool,
        sample_limit: u32,
    ) -> Result<SearchIndexDrift> {
        services
            .assets()
            .reconcile_search_index(repair, sample_limit)
            .await
    }
}
