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

fn default_snippet() -> bool {
    true
}

/// Query parameters for search
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flat breaks numeric deserialize)
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SearchParams {
    /// Page size — server-side default: 20 (page 1 = the 20 newest by
    /// updated_at DESC; follow next_cursor for more). Caller-controlled:
    /// no server-side maximum. Documented in the spec (issue b040d645 (d):
    /// the default was integrator-invisible).
    #[param(default = 20)]
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    pub q: Option<String>,
    /// Single tag filter (comma-separated for multiple: "type:document,status:draft")
    #[serde(default)]
    pub tag: Option<String>,
    /// Multiple tag filters. Every wire form is accepted (issues 12d3cb6a +
    /// b040d645 (c): serde_urlencoded has no sequence support — each value
    /// arrives as ONE string, so `Vec<String>` 400'd on every form the
    /// bridge emitted): a JSON-array string (the bridge's lossless form,
    /// `tags=%5B%22a%22%2C%22b%22%5D`), a comma-separated string (the `tag`
    /// param's documented convention, `tags=a,b`), or a true sequence.
    #[serde(default, deserialize_with = "deserialize_tags")]
    pub tags: Vec<String>,
    /// When true, return full Asset objects with complete content.
    /// Default: false (returns compact SearchResult with snippet)
    #[serde(default)]
    pub full_content: bool,
    /// When false, SKIP the content-fetch phase entirely — compact results
    /// carry no snippet field (issue 3fd2d848: callers that never render
    /// snippets — payam's channel walks — were paying one content-blob read
    /// per visible row on every search). Default: true (snippets fetched
    /// for the visible page).
    #[serde(default = "default_snippet")]
    pub snippet: bool,
}

/// Split a comma-separated tag list (the `tag` param's documented
/// convention); empty segments are dropped.
fn split_tag_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// The `tags` visitor — module level (an impl inside a generic fn cannot
/// use the fn's lifetime, E0401). Handles every wire form: sequences
/// (visit_seq) and strings (visit_str — JSON-array string or
/// comma-separated).
struct TagsVisitor;

impl<'de> serde::de::Visitor<'de> for TagsVisitor {
    type Value = Vec<String>;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a tag list: JSON array string, comma-separated tags, or a sequence")
    }

    fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut out = Vec::new();
        while let Some(s) = seq.next_element::<String>()? {
            out.push(s);
        }
        Ok(out)
    }

    fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let trimmed = s.trim();
        if trimmed.starts_with('[') {
            return serde_json::from_str::<Vec<String>>(trimmed).map_err(E::custom);
        }
        Ok(split_tag_list(trimmed))
    }
}

/// Deserialize the `tags` query param across every wire form (issues
/// 12d3cb6a + b040d645 (c)). serde_urlencoded (axum's Query backend) has no
/// sequence support: each field value arrives as ONE string (first
/// occurrence wins for repeated params), so `Vec<String>` 400'd on every
/// form — "invalid type: string, expected a sequence". Accepted forms:
/// JSON-array string (the bridge's lossless form), comma-separated string
/// (the `tag` param's documented convention), or a true sequence.
/// std::result::Result is explicit — crate::error::Result shadows it.
fn deserialize_tags<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    deserializer.deserialize_any(TagsVisitor)
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
        // Count first (borrowed filters) — the true total is cursor-
        // independent (issue b040d645 (b): PaginatedResponse.total existed
        // but was never populated; boards rendered page-local counts).
        let total = SearchService::count(tx.services(), params.q.as_deref(), &tag_filters).await?;
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
            total: Some(total),
        })?))
    } else {
        // Compact path (default): the page query carries NO content blobs —
        // the old SELECT read every matched body off disk per search and the
        // response discarded it (eea5bcff). Two-phase: page without content
        // → batch-Cedar filter → one content fetch for the visible page
        // (≤limit rows) → snippets from those bodies.
        // Count first (borrowed filters) — the true total is cursor-
        // independent (issue b040d645 (b)).
        let total = SearchService::count(tx.services(), params.q.as_deref(), &tag_filters).await?;
        let (assets, next_cursor) = SearchService::search_compact(
            tx.services(),
            params.q.as_deref(),
            tag_filters,
            params.limit,
            params.cursor.as_deref(),
        )
        .await?;

        let visible_assets = filter_visible(assets);

        // 3fd2d848: snippet=false skips the content fetch entirely —
        // callers that never render snippets stop paying one blob read per
        // visible row. SearchResult.snippet is Option — None serializes as
        // an absent field.
        let contents = if params.snippet {
            let ids: Vec<String> = visible_assets.iter().map(|a| a.id.clone()).collect();
            SearchService::get_contents(tx.services(), &ids).await?
        } else {
            std::collections::HashMap::new()
        };

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
            total: Some(total),
        })?))
    }
}

#[cfg(test)]
mod tags_wire_tests {
    use super::*;

    /// The wire forms the bridge and humans emit (issues 12d3cb6a +
    /// b040d645 (c)). serde_json drives the same visitor the query
    /// deserializer uses: arrays → visit_seq, strings → visit_str.
    #[derive(Debug, Deserialize)]
    struct Wire {
        #[serde(default, deserialize_with = "deserialize_tags")]
        tags: Vec<String>,
    }

    #[test]
    fn tags_json_array_string_parses_losslessly() {
        // The bridge's lossless form: tags=%5B%22type%3Adoc%22%2C%22scope%3Ax%22%5D
        let w: Wire = serde_json::from_str(r#"{"tags": "[\"type:doc\",\"scope:x\"]"}"#)
            .expect("JSON-array string parses");
        assert_eq!(w.tags, vec!["type:doc".to_string(), "scope:x".to_string()]);
    }

    #[test]
    fn tags_comma_separated_string_parses() {
        // The `tag` param's documented convention, on the `tags` key.
        let w: Wire = serde_json::from_str(r#"{"tags": "type:doc, scope:x"}"#)
            .expect("comma-separated parses");
        assert_eq!(w.tags, vec!["type:doc".to_string(), "scope:x".to_string()]);
    }

    #[test]
    fn tags_single_string_parses() {
        let w: Wire =
            serde_json::from_str(r#"{"tags": "status:pending"}"#).expect("single tag parses");
        assert_eq!(w.tags, vec!["status:pending".to_string()]);
    }

    #[test]
    fn tags_true_sequence_parses() {
        let w: Wire =
            serde_json::from_str(r#"{"tags": ["type:doc","scope:x"]}"#).expect("sequence parses");
        assert_eq!(w.tags, vec!["type:doc".to_string(), "scope:x".to_string()]);
    }

    #[test]
    fn tags_absent_defaults_empty() {
        let w: Wire = serde_json::from_str(r#"{}"#).expect("absent defaults");
        assert!(w.tags.is_empty());
    }

    #[test]
    fn tags_empty_string_is_empty_list() {
        let w: Wire = serde_json::from_str(r#"{"tags": ""}"#).expect("empty string parses");
        assert!(w.tags.is_empty());
    }

    #[test]
    fn tags_json_array_string_preserves_commas_in_values() {
        // The JSON form is lossless where comma-split would break: a tag
        // VALUE containing a comma survives.
        let w: Wire = serde_json::from_str(r#"{"tags": "[\"a,b\",\"c\"]"}"#)
            .expect("JSON array with embedded comma parses");
        assert_eq!(w.tags, vec!["a,b".to_string(), "c".to_string()]);
    }

    #[test]
    fn tags_malformed_json_array_is_an_error_not_garbage() {
        // A value that LOOKS like a JSON array but isn't must fail loud —
        // never silently become a garbage filter that matches nothing
        // (the lying-empty class).
        let result = serde_json::from_str::<Wire>(r#"{"tags": "[not json"}"#);
        assert!(result.is_err(), "malformed JSON array must error");
    }
}
