//! Asset handlers

use axum::{
    extract::{Path, Query},
    Json,
};
use pep::oidc::types::JwtClaims;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::auth::{AuthenticatedUser, Enrichment};
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, AdoptAssetRequest, Asset, CreateAssetRequest, PaginatedResponse, Tag,
    UpdateAssetRequest, UpdateAuthContextRequest,
};
use crate::service::AssetService;

fn default_limit() -> i64 {
    20
}

fn default_sort_by() -> String {
    "updated_at".to_string()
}

fn default_order() -> String {
    "desc".to_string()
}

/// Query parameters for listing assets
/// Note: pagination fields inlined to work around serde_urlencoded#33 (flatten breaks numeric deserialize)
#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ListAssetsParams {
    /// Page size — server-side default: 20 (page 1 = the 20 newest by
    /// sort order; follow next_cursor for more). Caller-controlled: no
    /// server-side maximum. Documented in the spec (issue b040d645 (d)).
    #[param(default = 20)]
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
    /// Filter by asset type tag value (e.g., "document", "concept", "idea")
    pub asset_type: Option<String>,
    /// Sort field: "created_at" or "updated_at" (default: "updated_at")
    #[serde(default = "default_sort_by")]
    pub sort_by: String,
    /// Sort order: "asc" or "desc" (default: "desc")
    #[serde(default = "default_order")]
    pub order: String,
}

/// Create a new asset
#[utoipa::path(
    post,
    path = "/api/assets",
    request_body = CreateAssetRequest,
    responses(
        (status = 200, description = "Asset created successfully", body = Asset),
        (status = 400, description = "Validation error"),
    ),
    tag = "assets",
)]
pub async fn create_asset(
    tx: TenantState,
    user: AuthenticatedUser,
    Json(request): Json<CreateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;
    let caller_groups = extract_caller_groups(&user);

    let asset = AssetService::create(tx.services(), request, user_id, &caller_groups).await?;
    Ok(Json(asset))
}

/// ADOPT an asset with a SPECIFIED id (same-uuid materialization — CPO
/// ruling 6d86ee70, the Path A adopt's first-class path).
///
/// Custody: PLATFORM-ADMIN only (role=admin) — an adopt is a
/// restore-with-custody, not a birth. Duplicate-barred (409 if the uuid
/// exists live or soft-deleted). Provenance REQUIRED (400 if absent) —
/// the call names the banked source. Audited ATOMICALLY with the
/// materialization: a failed audit write denies the adopt.
#[utoipa::path(
    post,
    path = "/api/assets/{id}/adopt",
    params(
        ("id" = String, Path, description = "The SPECIFIED uuid to materialize (the locked row's original id)"),
    ),
    request_body = AdoptAssetRequest,
    responses(
        (status = 200, description = "Asset adopted", body = Asset),
        (status = 400, description = "Validation error (empty title / missing provenance)"),
        (status = 403, description = "Custody: platform-admin only"),
        (status = 409, description = "Duplicate-barred: the uuid already exists in this store"),
    ),
    tag = "assets",
)]
pub async fn adopt_asset(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AdoptAssetRequest>,
) -> Result<Json<Asset>> {
    // CUSTODY (ruling 6d86ee70): platform-admin only.
    let claims = extract_claims(&user);
    let is_platform_admin = claims
        .extra
        .get("role")
        .map(|r| match r {
            serde_json::Value::String(s) => s == "admin",
            serde_json::Value::Array(arr) => {
                arr.iter().filter_map(|v| v.as_str()).any(|s| s == "admin")
            }
            _ => false,
        })
        .unwrap_or(false);
    if !is_platform_admin {
        return Err(ApiError::Forbidden(
            "Access denied: adopt custody is platform-admin only (ruling 6d86ee70)".to_string(),
        ));
    }

    let asset = AssetService::adopt(tx.services(), &id, request, &user.user_id).await?;
    Ok(Json(asset))
}

/// Extract the caller's `groups` claim for the born-empty guard (0e583f06).
/// Group-less principals (agent svcacct tokens, test construction) yield an
/// empty vec — the guard then defaults to an honest private band instead of
/// the born-mask black hole.
fn extract_caller_groups(user: &AuthenticatedUser) -> Vec<String> {
    user.claims_extra
        .as_ref()
        .and_then(|extra| extra.get("groups"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Get asset by ID
#[utoipa::path(
    get,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    responses(
        (status = 200, description = "Asset found", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn get_asset(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<Asset>> {
    let asset = AssetService::get(tx.services(), &id).await?;

    // Cedar enforcement: skip if no authorizer or asset has no auth_context
    if let Some(authorizer) = tx.authorizer() {
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            let resolved =
                crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;
            crate::cedar::enforcement::check_permission_resolved(
                tx.services(),
                authorizer,
                &claims,
                &resolved,
                tx.instance_id(),
                "View",
                &asset,
            )
            .await?;
        }
    }

    Ok(Json(asset))
}

/// Update an asset
#[utoipa::path(
    put,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = UpdateAssetRequest,
    responses(
        (status = 200, description = "Asset updated successfully", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn update_asset(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAssetRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    // Cedar enforcement on the current state of the asset
    if let Some(authorizer) = tx.authorizer() {
        let asset = AssetService::get(tx.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            let resolved =
                crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;
            crate::cedar::enforcement::check_permission_resolved(
                tx.services(),
                authorizer,
                &claims,
                &resolved,
                tx.instance_id(),
                "Edit",
                &asset,
            )
            .await?;
        }
    }

    let asset = AssetService::update(tx.services(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Delete an asset (soft delete)
#[utoipa::path(
    delete,
    path = "/api/assets/{id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    responses(
        (status = 200, description = "Asset deleted successfully"),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn delete_asset(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement
    if let Some(authorizer) = tx.authorizer() {
        let asset = AssetService::get(tx.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            let resolved =
                crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;
            crate::cedar::enforcement::check_permission_resolved(
                tx.services(),
                authorizer,
                &claims,
                &resolved,
                tx.instance_id(),
                "Delete",
                &asset,
            )
            .await?;
        }
    }

    AssetService::delete(tx.services(), &id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// List assets with pagination
/// Auth-degraded requests (enrichment failed → zero-attribute principal) answer
/// 503 auth_degraded instead of a lying 200-with-empty (issue 365be03f).
#[utoipa::path(
    get,
    path = "/api/assets",
    params(ListAssetsParams),
    responses(
        (status = 200, description = "List of assets", body = PaginatedResponse<Asset>),
        (status = 503, description = "Auth degraded — token claims could not be verified; body {error: auth_degraded, retryable: true}. Retry after re-authentication."),
    ),
    tag = "assets",
)]
pub async fn list_assets(
    tx: TenantState,
    user: AuthenticatedUser,
    Enrichment(enrichment): Enrichment,
    Query(params): Query<ListAssetsParams>,
) -> Result<Json<PaginatedResponse<Asset>>> {
    let claims = extract_claims(&user);
    let resolved = crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;

    // 365be03f: refuse lying absence — same contract as search. Phase 1b
    // (147a17fc): the merge makes the transport failure harmless — a
    // degraded TOKEN still carries PDT-native memberships, so Degraded
    // only refuses when BOTH sources are empty (zero = zero, honestly).
    if enrichment == crate::auth::AuthEnrichment::Degraded && resolved.effective_groups.is_empty() {
        return Err(ApiError::AuthDegraded);
    }
    let (assets, next_cursor) = AssetService::list(
        tx.services(),
        params.limit,
        params.cursor.as_deref(),
        params.asset_type.as_deref(),
        &params.sort_by,
        &params.order,
    )
    .await?;

    // The true total — cursor-independent, pre-visibility (issue b040d645
    // (b): PaginatedResponse.total existed but was never populated; boards
    // rendered page-local counts as if they were the whole set).
    let total = AssetService::count_list(tx.services(), params.asset_type.as_deref()).await?;

    // Cedar enforcement: filter out assets the user cannot view
    let filtered = if let Some(authorizer) = tx.authorizer() {
        crate::cedar::enforcement::filter_by_permission_resolved(
            tx.services(),
            authorizer,
            &claims,
            &resolved,
            tx.instance_id(),
            "View",
            assets,
        )
        .await?
    } else {
        assets
    };

    Ok(Json(PaginatedResponse {
        data: filtered,
        next_cursor,
        total: Some(total),
    }))
}

/// Add a tag to an asset
#[utoipa::path(
    post,
    path = "/api/assets/{id}/tags",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = AddTagRequest,
    responses(
        (status = 200, description = "Tag added successfully", body = Tag),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn add_tag(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddTagRequest>,
) -> Result<Json<Tag>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = tx.authorizer() {
        let asset = AssetService::get(tx.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            let resolved =
                crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;
            crate::cedar::enforcement::check_permission_resolved(
                tx.services(),
                authorizer,
                &claims,
                &resolved,
                tx.instance_id(),
                "Tag",
                &asset,
            )
            .await?;
        }
    }

    let tag = AssetService::add_tag(tx.services(), &id, request, user_id).await?;
    Ok(Json(tag))
}

/// Remove a tag from an asset
#[utoipa::path(
    delete,
    path = "/api/assets/{id}/tags/{tag_id}",
    params(
        ("id" = String, Path, description = "Asset ID"),
        ("tag_id" = String, Path, description = "Tag ID to remove"),
    ),
    responses(
        (status = 200, description = "Tag removed successfully"),
        (status = 404, description = "Asset or tag not found"),
    ),
    tag = "assets",
)]
pub async fn remove_tag(
    tx: TenantState,
    user: AuthenticatedUser,
    Path((id, tag_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let user_id = &user.user_id;

    // Cedar enforcement for Tag action
    if let Some(authorizer) = tx.authorizer() {
        let asset = AssetService::get(tx.services(), &id).await?;
        if crate::cedar::enforcement::should_enforce_cedar(&asset) {
            let claims = extract_claims(&user);
            let resolved =
                crate::cedar::enforcement::resolve_instance_authz(tx.services(), &claims).await;
            crate::cedar::enforcement::check_permission_resolved(
                tx.services(),
                authorizer,
                &claims,
                &resolved,
                tx.instance_id(),
                "Tag",
                &asset,
            )
            .await?;
        }
    }

    AssetService::remove_tag(tx.services(), &id, &tag_id, user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

/// Update the authorization context of an asset (Cedar visibility/ownership management)
#[utoipa::path(
    put,
    path = "/api/assets/{id}/auth-context",
    params(
        ("id" = String, Path, description = "Asset ID"),
    ),
    request_body = UpdateAuthContextRequest,
    responses(
        (status = 200, description = "Auth context updated successfully", body = Asset),
        (status = 404, description = "Asset not found"),
    ),
    tag = "assets",
)]
pub async fn update_auth_context(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<UpdateAuthContextRequest>,
) -> Result<Json<Asset>> {
    let user_id = &user.user_id;

    let asset = AssetService::update_auth_context(tx.services(), &id, request, user_id).await?;
    Ok(Json(asset))
}

/// Extract JwtClaims from the AuthenticatedUser.
///
/// Reconstructs JwtClaims preserving the original `extra` map (role, groups, etc.)
/// that was extracted from the real JWT by the auth middleware.
fn extract_claims(user: &AuthenticatedUser) -> JwtClaims {
    user.to_cedar_claims()
}
