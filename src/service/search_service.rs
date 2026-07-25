//! Search service

use crate::error::Result;
use crate::models::Asset;
use crate::service::Services;

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
        services.assets().search(query, tag_filters, limit, cursor).await
    }
}
