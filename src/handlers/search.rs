//! Search handlers

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::Result;
use crate::models::{Asset, PaginatedResponse};
use crate::service::{SearchService, Services};

fn default_limit() -> i64 {
    20
}

/// Query parameters for search
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flat breaks numeric deserialize)
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    pub q: Option<String>,
    /// Single tag filter (comma-separated for multiple: "type:document,status:draft")
    #[serde(default)]
    pub tag: Option<String>,
    /// Multiple tag filters (repeated param: tag=type:doc&tag=status:draft)
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Search assets
pub async fn search(
    State(services): State<Services>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    // Combine single tag with tags array
    let mut all_tags = params.tags.clone();
    if let Some(tag) = &params.tag {
        all_tags.push(tag.clone());
    }
    
    // Parse tag filters (format: "category:value")
    // Supports both exploded (tags=a:b&tags=c:d) and comma-separated (tags=a:b,c:d) formats
    let tag_filters: Vec<(String, String)> = all_tags
        .iter()
        .flat_map(|t| t.split(','))
        .filter_map(|t| {
            let parts: Vec<&str> = t.splitn(2, ':').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_string()))
            } else {
                None
            }
        })
        .collect();

    let (assets, next_cursor) = SearchService::search(
        services.db(),
        params.q.as_deref(),
        tag_filters,
        params.limit,
        params.cursor.as_deref(),
    )
    .await?;

    Ok(Json(PaginatedResponse {
        data: assets,
        next_cursor,
        total: None,
    }))
}
