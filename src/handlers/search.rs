//! Search handlers

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::error::Result;
use crate::models::{Asset, PaginatedResponse, PaginationParams};
use crate::service::{SearchService, Services};

/// Query parameters for search
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub q: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Search assets
pub async fn search(
    State(services): State<Services>,
    Query(params): Query<SearchParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    // Parse tag filters (format: "category:value")
    let tag_filters: Vec<(String, String)> = params
        .tags
        .iter()
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
        params.pagination.limit,
        params.pagination.cursor.as_deref(),
    )
    .await?;

    Ok(Json(PaginatedResponse {
        data: assets,
        next_cursor,
        total: None,
    }))
}
