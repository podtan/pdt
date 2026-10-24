//! Search handlers

use axum::{extract::Query, Json};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::auth::{AuthenticatedUser, Enrichment};
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::models::{generate_snippet, PaginatedResponse, SearchResult, TagSummary};
use crate::service::SearchService;

fn default_limit() -> i64 {
    20
}

/// Query parameters for search
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flat breaks numeric deserialize)
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
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
/// Auth-degraded requests (enrichment failed → zero-attribute principal) answer
/// 503 auth_degraded instead of a lying 200-with-empty (issue 365be03f).
#[utoipa::path(
    get,
    path = "/api/search",
    params(SearchParams),
    responses(
        (status = 200, description = "Search results (compact SearchResult by default, full Asset when full_content=true)"),
        (status = 503, description = "Auth degraded — token claims could not be verified; body {error: auth_degraded, retryable: true}. Retry after re-authentication."),
    ),
    tag = "search",
)]
pub async fn search(
    tx: TenantState,
    user: AuthenticatedUser,
    Enrichment(enrichment): Enrichment,
    Query(params): Query<SearchParams>,
) -> Result<Json<serde_json::Value>> {
    // 365be03f: a degraded principal carries no authorization material; the
    // Cedar visibility filter would drop every row and answer 200-with-empty
    // — indistinguishable from genuine absence. Refuse before any query.
    if enrichment == crate::auth::AuthEnrichment::Degraded {
        return Err(ApiError::AuthDegraded);
    }
    let claims = crate::cedar::enforcement::extract_claims_for_cedar(&user);
    let tag_filters = parse_tag_filters(&params);

    // Batched Cedar filter (eea5bcff): one eval per distinct band tuple
    // instead of one per row — a 904-row inbox was 904 Cedar evals per view.
    let filter_visible = |assets: Vec<crate::models::Asset>| {
        if let Some(authorizer) = tx.authorizer() {
            crate::cedar::enforcement::filter_by_permission_batched(
                authorizer, &claims, "View", assets,
            )
        } else {
            assets
        }
    };

    if params.full_content {
        // Legacy behavior: full assets, content included in the page query.
        let (assets, next_cursor) = SearchService::search(
            tx.services(),
            params.q.as_deref(),
            tag_filters,
            params.limit,
            params.cursor.as_deref(),
        )
        .await?;

        let visible_assets = filter_visible(assets);

        Ok(Json(serde_json::to_value(PaginatedResponse {
            data: visible_assets,
            next_cursor,
            total: None,
        })?))
    } else {
        // Compact path (default): the page query carries NO content blobs —
        // the old SELECT read every matched body off disk per search and the
        // response discarded it (eea5bcff). Two-phase: page without content
        // → batch-Cedar filter → one content fetch for the visible page
        // (≤limit rows) → snippets from those bodies.
        let (assets, next_cursor) = SearchService::search_compact(
            tx.services(),
            params.q.as_deref(),
            tag_filters,
            params.limit,
            params.cursor.as_deref(),
        )
        .await?;

        let visible_assets = filter_visible(assets);

        let ids: Vec<String> = visible_assets.iter().map(|a| a.id.clone()).collect();
        let contents = SearchService::get_contents(tx.services(), &ids).await?;

        let results: Vec<SearchResult> = visible_assets
            .into_iter()
            .map(|a| {
                let content = contents.get(&a.id).cloned().unwrap_or_default();
                SearchResult {
                    id: a.id,
                    title: a.title,
                    snippet: generate_snippet(&content, 200),
                    tags: a
                        .tags
                        .into_iter()
                        .map(|t| TagSummary {
                            category: t.category,
                            value: t.value,
                        })
                        .collect(),
                    updated_at: a.updated_at,
                    created_at: a.created_at,
                }
            })
            .collect();

        Ok(Json(serde_json::to_value(PaginatedResponse {
            data: results,
            next_cursor,
            total: None,
        })?))
    }
}
