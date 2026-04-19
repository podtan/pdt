//! Search handlers

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::AppState;
use crate::error::Result;
use crate::models::{
    generate_snippet, PaginatedResponse, SearchResult, TagSummary,
};
use crate::service::SearchService;

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
    /// When true, return full Asset objects with complete content.
    /// Default: false (returns compact SearchResult with snippet)
    #[serde(default)]
    pub full_content: bool,
}

/// Internal helper to build the common tag_filters from params
fn parse_tag_filters(params: &SearchParams) -> Vec<(String, String)> {
    let mut all_tags = params.tags.clone();
    if let Some(tag) = &params.tag {
        all_tags.push(tag.clone());
    }

    all_tags
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
        .collect()
}

/// Search assets — returns compact SearchResult by default, full Asset when full_content=true
/// Cedar filtering: assets the user cannot View are excluded from results.
pub async fn search(
    State(state): State<AppState>,
    user: AuthenticatedUser,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>> {
    let claims = crate::cedar::enforcement::extract_claims_for_cedar(&user);
    let tag_filters = parse_tag_filters(&params);

    let (assets, next_cursor) = SearchService::search(
        state.services().db(),
        params.q.as_deref(),
        tag_filters,
        params.limit,
        params.cursor.as_deref(),
    )
    .await?;

    // Filter assets by Cedar View permission
    let visible_assets = if let Some(authorizer) = state.authorizer() {
        crate::cedar::enforcement::filter_by_permission(
            authorizer,
            &claims,
            "View",
            assets,
        )
    } else {
        assets
    };

    if params.full_content {
        // Legacy behavior: return full assets
        Ok(Json(serde_json::to_value(PaginatedResponse {
            data: visible_assets,
            next_cursor,
            total: None,
        })?))
    } else {
        // New default: return compact search results
        let results: Vec<SearchResult> = visible_assets
            .into_iter()
            .map(|a| SearchResult {
                id: a.id,
                title: a.title,
                snippet: generate_snippet(a.content.as_deref().unwrap_or(""), 200),
                tags: a
                    .tags
                    .into_iter()
                    .map(|t| TagSummary {
                        category: t.category,
                        value: t.value,
                    })
                    .collect(),
                updated_at: a.updated_at,
            })
            .collect();

        Ok(Json(serde_json::to_value(PaginatedResponse {
            data: results,
            next_cursor,
            total: None,
        })?))
    }
}
