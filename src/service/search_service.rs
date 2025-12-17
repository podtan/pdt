//! Search service

use crate::db::Database;
use crate::error::Result;
use crate::models::Asset;
use crate::repository::AssetRepository;

/// Service for search business logic
pub struct SearchService;

impl SearchService {
    /// Search assets by text and tags
    pub async fn search(
        db: &Database,
        query: Option<&str>,
        tag_filters: Vec<(String, String)>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        AssetRepository::search(db, query, tag_filters, limit, cursor).await
    }
}
