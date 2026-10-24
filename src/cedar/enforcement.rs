//! Cedar enforcement logic for PDT
//!
//! This module provides the core enforcement functions that decide whether
//! Cedar authorization should be applied to a given asset, and evaluates
//! authorization requests against Cedar policies.
//!
//! ## Grandfathering Strategy
//!
//! Assets without `auth_context` (400+ existing docs) bypass Cedar enforcement
//! and default to open access. Only assets with `auth_context` go through Cedar.

use std::sync::Arc;

use axum::extract::{FromRef, FromRequestParts};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use cedar_policy::{Context, Entities, Request};
use pep::cedar::CedarAuthorizer;
use pep::oidc::types::JwtClaims;

use crate::auth::AuthenticatedUser;
use crate::error::ApiError;
use crate::models::Asset;
use crate::service::Services;
use crate::tenant::TenantPoolManager;

/// The raw application state type passed to `Router::with_state()`.
/// This is a tuple so axum can extract different parts via `FromRef`.
pub type AppRawState = (TenantPoolManager, Option<Arc<CedarAuthorizer>>);

/// Application state shared across all handlers.
/// Implements `FromRef` so axum can extract it from the tuple state.
///
/// NOTE: This resolves to the GLOBAL services. For tenant-aware services,
/// use the `TenantState` extractor instead.
#[derive(Clone)]
pub struct AppState {
    pub services: Services,
    pub authorizer: Option<Arc<CedarAuthorizer>>,
}

impl AppState {
    /// Get a reference to the services
    pub fn services(&self) -> &Services {
        &self.services
    }

    /// Get a reference to the Cedar authorizer (if enabled)
    pub fn authorizer(&self) -> Option<&CedarAuthorizer> {
        self.authorizer.as_ref().map(|v| v.as_ref())
    }
}

/// Tenant-aware state extractor.
///
/// Reads the `X-Instance-Id` header from the request and resolves the correct
/// `Services` (per-instance SQLite or global). Use this in handlers that need
/// tenant routing:
///
/// ```rust,ignore
/// pub async fn create_asset(
///     tx: TenantState,
///     user: AuthenticatedUser,
///     Json(request): Json<CreateAssetRequest>,
/// ) -> Result<Json<Asset>> {
///     let services = &tx.services;
///     // ...
/// }
/// ```
#[derive(Clone)]
pub struct TenantState {
    pub services: Services,
    pub authorizer: Option<Arc<CedarAuthorizer>>,
    pub instance_id: Option<String>,
}

impl TenantState {
    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn authorizer(&self) -> Option<&CedarAuthorizer> {
        self.authorizer.as_ref().map(|v| v.as_ref())
    }

    pub fn instance_id(&self) -> Option<&str> {
        self.instance_id.as_deref()
    }
}

/// Implement FromRequestParts on the concrete AppRawState type.
impl<S> FromRequestParts<S> for TenantState
where
    S: Send + Sync,
    TenantPoolManager: FromRef<S>,
    AuthorizerRef: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        use axum::http::StatusCode;

        // Extract X-Instance-Id header
        let instance_id = parts
            .headers
            .get("X-Instance-Id")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        let manager = TenantPoolManager::from_ref(state);
        let auth_ref = AuthorizerRef::from_ref(state);

        let services = manager
            .get_services(instance_id.as_deref())
            .await
            .map_err(|e| {
                tracing::error!("Failed to resolve tenant services: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Failed to resolve tenant: {}", e),
                )
                    .into_response()
            })?;

        Ok(TenantState {
            services,
            authorizer: auth_ref.0,
            instance_id,
        })
    }
}

/// Wrapper for the optional Cedar authorizer so we can implement `FromRef`.
#[derive(Clone)]
pub struct AuthorizerRef(pub Option<Arc<CedarAuthorizer>>);

impl AuthorizerRef {
    pub fn as_ref(&self) -> Option<&CedarAuthorizer> {
        self.0.as_ref().map(|v| v.as_ref())
    }
}

/// Implement FromRef for backward-compatible AppState (global services).
impl FromRef<AppRawState> for AppState {
    fn from_ref(state: &AppRawState) -> Self {
        AppState {
            services: state.0.global().clone(),
            authorizer: state.1.clone(),
        }
    }
}

impl FromRef<AppRawState> for TenantPoolManager {
    fn from_ref(state: &AppRawState) -> Self {
        state.0.clone()
    }
}

impl FromRef<AppRawState> for AuthorizerRef {
    fn from_ref(state: &AppRawState) -> Self {
        AuthorizerRef(state.1.clone())
    }
}

/// Build JwtClaims from AuthenticatedUser for Cedar evaluation.
/// This is used by handlers that need claims for Cedar enforcement
/// (e.g., search, list filtering) but use AuthenticatedUser for extraction.
///
/// Propagates the original `extra` map (role, groups, etc.) from the real JWT.
pub fn extract_claims_for_cedar(user: &AuthenticatedUser) -> JwtClaims {
    user.to_cedar_claims()
}

/// Check whether Cedar authorization should be enforced for this asset.
///
/// Assets with `auth_context` present are Cedar-enforced.
/// Assets without `auth_context` (grandfathered) bypass enforcement entirely.
pub fn should_enforce_cedar(asset: &Asset) -> bool {
    asset.auth_context.is_some()
}

/// Check Cedar permission — returns 403 if denied.
///
/// This is the main enforcement function used by handlers.
/// For grandfathered assets (no auth_context), always allows.
/// For Cedar-enforced assets, evaluates the policy.
pub fn check_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset: &Asset,
) -> Result<(), ApiError> {
    if !should_enforce_cedar(asset) {
        return Ok(()); // Grandfathered — open access
    }

    let allowed = evaluate_permission(authorizer, claims, action, asset)?;
    if !allowed {
        tracing::warn!(
            "Cedar denied {} on asset {} for user {}",
            action,
            asset.id,
            claims.sub
        );
        return Err(ApiError::Forbidden(format!(
            "Access denied: you do not have '{}' permission on asset '{}'",
            action, asset.id
        )));
    }
    Ok(())
}

/// Evaluate a Cedar authorization request for a specific asset.
///
/// Returns `Ok(true)` if the action is allowed, `Ok(false)` if denied.
/// Returns `Err` only if there is an entity-building error.
pub fn evaluate_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset: &Asset,
) -> Result<bool, ApiError> {
    let principal = pep::cedar::build_principal_uid(claims).map_err(|e| {
        tracing::warn!("Cedar principal build failed: {}", e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    let action_uid = pep::cedar::build_action_uid(action).map_err(|e| {
        tracing::warn!("Cedar action build failed: {}", e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    let resource = crate::cedar::entity::build_asset_resource_uid(asset).map_err(|e| {
        tracing::warn!("Cedar resource build failed for asset {}: {}", asset.id, e);
        ApiError::Forbidden(format!("Authorization error: {}", e))
    })?;

    // Build entities so Cedar can evaluate attributes
    let asset_entity = crate::cedar::entity::asset_to_cedar_entity(asset);
    let principal_entity = crate::cedar::entity::user_to_cedar_principal(claims);

    let entities =
        Entities::from_entities([asset_entity, principal_entity], None).map_err(|e| {
            tracing::warn!("Cedar entities build failed: {}", e);
            ApiError::Forbidden(format!("Authorization error: {}", e))
        })?;

    let request =
        Request::new(principal, action_uid, resource, Context::empty(), None).map_err(|e| {
            tracing::warn!("Cedar request build failed: {}", e);
            ApiError::Forbidden(format!("Authorization error: {}", e))
        })?;

    let response = authorizer.is_allowed_with_entities(&request, &entities);

    Ok(response.allowed())
}

/// Check if the current user can perform an action on an asset (by ID, fetching from DB).
///
/// Used by relation handlers that need to verify permission on related assets
/// but don't already have the full asset loaded.
pub async fn check_asset_permission_by_id(
    services: &Services,
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    asset_id: &str,
) -> Result<(), ApiError> {
    let asset = services.assets().get_by_id(asset_id).await?;
    if let Some(authz) = check_permission(authorizer, claims, action, &asset).err() {
        return Err(authz);
    }
    Ok(())
}

/// Check if user can perform an action involving two assets (e.g., creating a relation).
///
/// Both the source and target assets must be readable by the user.
pub async fn check_relation_permission(
    services: &Services,
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    from_asset_id: &str,
    to_asset_id: &str,
) -> Result<(), ApiError> {
    check_asset_permission_by_id(services, authorizer, claims, "View", from_asset_id).await?;
    check_asset_permission_by_id(services, authorizer, claims, action, to_asset_id).await?;
    Ok(())
}

/// Evaluate Cedar authorization for a list of assets, filtering out denied ones.
///
/// Returns only the assets that the given user is allowed to perform the action on.
/// Grandfathered assets (no `auth_context`) are always included.
pub fn filter_by_permission(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    assets: Vec<Asset>,
) -> Vec<Asset> {
    assets
        .into_iter()
        .filter(|asset| {
            if !should_enforce_cedar(asset) {
                return true; // Grandfathered — always include
            }
            match evaluate_permission(authorizer, claims, action, asset) {
                Ok(allowed) => allowed,
                Err(e) => {
                    tracing::warn!(
                        "Cedar evaluation error for asset {}: {}, denying access",
                        asset.id,
                        e
                    );
                    false
                }
            }
        })
        .collect()
}

/// Band key: the full Cedar-entity input tuple for one asset.
///
/// `asset_to_cedar_entity` builds the entity from created_by, the
/// auth_context band fields (visibility, confidentiality, owner_groups),
/// and the status/asset_type tag values. Assets sharing all of those build
/// IDENTICAL Cedar entities — the only per-asset variant left is the
/// resource uid, which no policy references (pinned by the
/// `batch_vs_per_row_equivalence` test).
fn cedar_band_key(asset: &Asset) -> String {
    let (vis, conf, groups) = match &asset.auth_context {
        Some(ctx) => {
            let mut g = ctx.owner_groups.clone();
            g.sort();
            (
                format!("{}", ctx.visibility),
                format!("{}", ctx.confidentiality),
                g.join(","),
            )
        }
        None => (String::new(), String::new(), String::new()),
    };
    let status = asset
        .tags
        .iter()
        .find(|t| t.category == "status")
        .map(|t| t.value.clone())
        .unwrap_or_default();
    let asset_type = asset
        .tags
        .iter()
        .find(|t| t.category == "asset_type")
        .map(|t| t.value.clone())
        .unwrap_or_default();
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
        asset.created_by, vis, conf, groups, status, asset_type
    )
}

/// Batched variant of `filter_by_permission` (issue eea5bcff): groups
/// assets by their full Cedar-input tuple and evaluates Cedar ONCE per
/// distinct tuple, applying the verdict to every asset in the group.
///
/// The search handler previously ran `evaluate_permission` per row — a
/// 904-row inbox was 904 Cedar evals per view, O(rows) authz on every read.
/// Mailbox-class reads (rows share one band) collapse to 1–5 evals.
///
/// Semantics-preserving by construction: identical Cedar inputs produce
/// identical decisions. An evaluation error denies the whole band — the
/// same fail-closed outcome the per-row filter applies per asset.
pub fn filter_by_permission_batched(
    authorizer: &CedarAuthorizer,
    claims: &JwtClaims,
    action: &str,
    assets: Vec<Asset>,
) -> Vec<Asset> {
    use std::collections::HashMap;

    // One verdict per distinct band; grandfathered assets skip eval entirely.
    let mut verdicts: HashMap<String, bool> = HashMap::new();
    for asset in &assets {
        if !should_enforce_cedar(asset) {
            continue;
        }
        let key = cedar_band_key(asset);
        if verdicts.contains_key(&key) {
            continue;
        }
        let verdict = match evaluate_permission(authorizer, claims, action, asset) {
            Ok(allowed) => allowed,
            Err(e) => {
                tracing::warn!(
                    "Cedar evaluation error for band (representative asset {}): {}, denying access",
                    asset.id,
                    e
                );
                false
            }
        };
        verdicts.insert(key, verdict);
    }

    assets
        .into_iter()
        .filter(|asset| {
            if !should_enforce_cedar(asset) {
                return true; // Grandfathered — always include
            }
            verdicts
                .get(&cedar_band_key(asset))
                .copied()
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod created_by_delete_tests {
    //! Delete-gap regression tests (Farzan ruling d130b9b5, 2026-10-17):
    //! created-by delete ALLOW, non-created-by non-admin DENY, admin ALLOW.
    //! The class: a user who could create a workspace but could never delete
    //! it (Cedar permitted Delete only for role=admin) — an orphan factory
    //! with a 500-on-403 wrapper in tocpi.

    use super::*;
    use crate::models::AuthContext;
    use pep::cedar::CedarConfig;
    use std::path::PathBuf;

    fn test_authorizer() -> CedarAuthorizer {
        let config = CedarConfig {
            policy_path: PathBuf::from("/nonexistent/policies.cedar"),
            schema_path: None,
            entities_path: None,
            default_decision: Default::default(),
            validate_on_load: true,
            policy_store_url: None,
            policy_store_token: None,
            embedded_policy: Some(include_str!("../../policies/rbac.cedar")),
            embedded_schema: Some(include_str!("../../policies/schema.cedarschema")),
        };
        CedarAuthorizer::new(config).expect("embedded policy loads")
    }

    fn claims_for(sub: &str, role: &str) -> JwtClaims {
        let mut extra = std::collections::HashMap::new();
        extra.insert(
            "role".to_string(),
            serde_json::Value::String(role.to_string()),
        );
        extra.insert(
            "groups".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(
                "unrelated-group".to_string(),
            )]),
        );
        JwtClaims {
            sub: sub.to_string(),
            iss: "test-issuer".to_string(),
            aud: None,
            exp: 9999999999,
            iat: None,
            email: Some(format!("{}@example.com", sub)),
            name: None,
            preferred_username: None,
            extra,
        }
    }

    fn workspace_asset(created_by: &str) -> Asset {
        Asset {
            id: "ws-test-1".to_string(),
            title: "Test Workspace".to_string(),
            content: None,
            tags: vec![],
            metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: created_by.to_string(),
            updated_by: created_by.to_string(),
            deleted_at: None,
            auth_context: Some(AuthContext {
                visibility: "team".to_string(),
                owner_groups: vec!["ws-ws-test-1-admins@idp.tanbal.ir".to_string()],
                confidentiality: String::new(),
            }),
        }
    }

    #[tokio::test]
    async fn created_by_delete_allowed() {
        let authorizer = test_authorizer();
        let asset = workspace_asset("alice");
        let claims = claims_for("alice", "user");
        let allowed = evaluate_permission(&authorizer, &claims, "Delete", &asset)
            .expect("evaluation succeeds");
        assert!(allowed, "creator must be able to delete their own asset");
    }

    #[tokio::test]
    async fn non_created_by_non_admin_delete_denied() {
        let authorizer = test_authorizer();
        let asset = workspace_asset("alice");
        let claims = claims_for("bob", "user");
        let allowed = evaluate_permission(&authorizer, &claims, "Delete", &asset)
            .expect("evaluation succeeds");
        assert!(!allowed, "non-creator non-admin must NOT be able to delete");
    }

    #[tokio::test]
    async fn admin_delete_still_allowed() {
        let authorizer = test_authorizer();
        let asset = workspace_asset("alice");
        let claims = claims_for("super", "admin");
        let allowed = evaluate_permission(&authorizer, &claims, "Delete", &asset)
            .expect("evaluation succeeds");
        assert!(allowed, "admin (super_admin) delete must keep working");
    }

    #[tokio::test]
    async fn created_by_edit_unchanged() {
        let authorizer = test_authorizer();
        let asset = workspace_asset("alice");
        let claims = claims_for("alice", "user");
        let allowed =
            evaluate_permission(&authorizer, &claims, "Edit", &asset).expect("evaluation succeeds");
        assert!(allowed, "created_by Edit policy must keep working");
    }
}

#[cfg(test)]
mod team_sub_keyed_tests {
    //! Team permit sub/SPN-keyed arm regression tests (CPO ruling eda88e15,
    //! 2026-10-23, branch (a)): exchanged/machine tokens carry NO groups
    //! claim (6c8efe55 class) — a byte-exact principal-sub (or SPN) match in
    //! owner_groups must grant View AND Edit independent of groups-claim
    //! enrichment. The deny hit admins and agents alike; both directions
    //! pinned here (sub-match ALLOW, non-match DENY — no widening).

    use super::*;
    use crate::models::AuthContext;
    use pep::cedar::CedarConfig;
    use std::path::PathBuf;

    fn test_authorizer() -> CedarAuthorizer {
        let config = CedarConfig {
            policy_path: PathBuf::from("/nonexistent/policies.cedar"),
            schema_path: None,
            entities_path: None,
            default_decision: Default::default(),
            validate_on_load: true,
            policy_store_url: None,
            policy_store_token: None,
            embedded_policy: Some(include_str!("../../policies/rbac.cedar")),
            embedded_schema: Some(include_str!("../../policies/schema.cedarschema")),
        };
        CedarAuthorizer::new(config).expect("embedded policy loads")
    }

    /// Exchanged/machine token class: NO groups claim in extra at all.
    fn claims_no_groups(sub: &str, role: &str, preferred_username: Option<&str>) -> JwtClaims {
        let mut extra = std::collections::HashMap::new();
        extra.insert(
            "role".to_string(),
            serde_json::Value::String(role.to_string()),
        );
        JwtClaims {
            sub: sub.to_string(),
            iss: "https://idp.tanbal.ir".to_string(),
            aud: None,
            exp: 9999999999,
            iat: None,
            email: Some(format!("{}@example.com", sub)),
            name: None,
            preferred_username: preferred_username.map(|s| s.to_string()),
            extra,
        }
    }

    fn team_asset(owner_groups: Vec<String>) -> Asset {
        Asset {
            id: "team-asset-1".to_string(),
            title: "Team Asset".to_string(),
            content: None,
            tags: vec![],
            metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: "creator-1".to_string(),
            updated_by: "creator-1".to_string(),
            deleted_at: None,
            auth_context: Some(AuthContext {
                visibility: "team".to_string(),
                owner_groups,
                confidentiality: String::new(),
            }),
        }
    }

    /// THE CLASS (ruling acceptance): exchanged token with no groups claim,
    /// sub ∈ owner_groups → View ALLOW.
    #[tokio::test]
    async fn sub_in_owner_groups_view_allowed_without_groups_claim() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["30309a40-2588-4cb6-8bda-0f7ccafd8d95".to_string()]);
        let claims = claims_no_groups("30309a40-2588-4cb6-8bda-0f7ccafd8d95", "user", None);
        let allowed =
            evaluate_permission(&authorizer, &claims, "View", &asset).expect("evaluation succeeds");
        assert!(
            allowed,
            "byte-exact sub match in owner_groups must grant View for a groups-less token"
        );
    }

    /// Edit carries the identical hole (root-cause 67ab6a29) — same arm.
    #[tokio::test]
    async fn sub_in_owner_groups_edit_allowed_without_groups_claim() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["30309a40-2588-4cb6-8bda-0f7ccafd8d95".to_string()]);
        let claims = claims_no_groups("30309a40-2588-4cb6-8bda-0f7ccafd8d95", "user", None);
        let allowed =
            evaluate_permission(&authorizer, &claims, "Edit", &asset).expect("evaluation succeeds");
        assert!(
            allowed,
            "byte-exact sub match in owner_groups must grant Edit for a groups-less token"
        );
    }

    /// Cross-principal attestation, non-admin direction (ruling: the deny
    /// hit admins and agents alike): role=user, no groups, sub match → ALLOW.
    #[tokio::test]
    async fn non_admin_principal_sub_match_allowed() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["agent-abc-123".to_string()]);
        let claims = claims_no_groups("agent-abc-123", "user", None);
        let allowed =
            evaluate_permission(&authorizer, &claims, "View", &asset).expect("evaluation succeeds");
        assert!(
            allowed,
            "non-admin principal with sub ∈ owner_groups must be allowed"
        );
    }

    /// NO WIDENING: sub ∉ owner_groups and no groups claim → still DENY.
    #[tokio::test]
    async fn sub_not_in_owner_groups_still_denied() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["someone-elses-sub".to_string()]);
        let claims = claims_no_groups("30309a40-2588-4cb6-8bda-0f7ccafd8d95", "user", None);
        let allowed =
            evaluate_permission(&authorizer, &claims, "View", &asset).expect("evaluation succeeds");
        assert!(
            !allowed,
            "sub not in owner_groups and no groups claim must stay denied — no widening"
        );
    }

    /// SPN-keyed half of the ruling: owner_groups holding the principal's
    /// SPN form (<account>@<issuer-domain>) matches without a groups claim.
    #[tokio::test]
    async fn spn_in_owner_groups_allowed_without_groups_claim() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["zobde@idp.tanbal.ir".to_string()]);
        let claims = claims_no_groups(
            "30309a40-2588-4cb6-8bda-0f7ccafd8d95",
            "user",
            Some("zobde"),
        );
        let allowed =
            evaluate_permission(&authorizer, &claims, "View", &asset).expect("evaluation succeeds");
        assert!(allowed, "SPN-form match in owner_groups must grant View");
    }

    /// Existing groups-claim path regression guard: enrichment still works.
    #[tokio::test]
    async fn groups_claim_path_still_works() {
        let authorizer = test_authorizer();
        let asset = team_asset(vec!["developers".to_string()]);
        let mut claims = claims_no_groups("30309a40-2588-4cb6-8bda-0f7ccafd8d95", "user", None);
        claims.extra.insert(
            "groups".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String("developers".to_string())]),
        );
        let allowed =
            evaluate_permission(&authorizer, &claims, "View", &asset).expect("evaluation succeeds");
        assert!(allowed, "groups-claim matching must keep working");
    }
}

#[cfg(test)]
mod batched_filter_tests {
    //! Batch-vs-per-row equivalence regression tests (issue eea5bcff,
    //! 2026-10-24): `filter_by_permission_batched` must produce the
    //! IDENTICAL visible set to `filter_by_permission` over a mixed-band
    //! fixture. The batch groups assets by their full Cedar-input tuple
    //! (created_by + auth_context band + status/asset_type tag values) and
    //! evaluates once per group — semantics-preserving by construction; the
    //! only per-asset variant left is the resource uid, which no policy
    //! references. These tests pin that invariant.

    use super::*;
    use crate::models::{AuthContext, Tag};
    use pep::cedar::CedarConfig;
    use std::path::PathBuf;

    fn test_authorizer() -> CedarAuthorizer {
        let config = CedarConfig {
            policy_path: PathBuf::from("/nonexistent/policies.cedar"),
            schema_path: None,
            entities_path: None,
            default_decision: Default::default(),
            validate_on_load: true,
            policy_store_url: None,
            policy_store_token: None,
            embedded_policy: Some(include_str!("../../policies/rbac.cedar")),
            embedded_schema: Some(include_str!("../../policies/schema.cedarschema")),
        };
        CedarAuthorizer::new(config).expect("embedded policy loads")
    }

    fn claims_for(sub: &str, role: &str) -> JwtClaims {
        let mut extra = std::collections::HashMap::new();
        extra.insert(
            "role".to_string(),
            serde_json::Value::String(role.to_string()),
        );
        JwtClaims {
            sub: sub.to_string(),
            iss: "test-issuer".to_string(),
            aud: None,
            exp: 9999999999,
            iat: None,
            email: Some(format!("{}@example.com", sub)),
            name: None,
            preferred_username: None,
            extra,
        }
    }

    fn tag(category: &str, value: &str) -> Tag {
        Tag {
            id: uuid::Uuid::new_v4().to_string(),
            category: category.to_string(),
            value: value.to_string(),
            added_by: "test".to_string(),
            added_at: chrono::Utc::now(),
        }
    }

    fn asset(
        id: &str,
        created_by: &str,
        auth_context: Option<AuthContext>,
        tags: Vec<Tag>,
    ) -> Asset {
        Asset {
            id: id.to_string(),
            title: format!("Asset {}", id),
            content: None,
            tags,
            metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            created_by: created_by.to_string(),
            updated_by: created_by.to_string(),
            deleted_at: None,
            auth_context,
        }
    }

    fn visible_ids(assets: &[Asset]) -> Vec<String> {
        assets.iter().map(|a| a.id.clone()).collect()
    }

    #[tokio::test]
    async fn batch_vs_per_row_equivalence_mixed_bands() {
        let authorizer = test_authorizer();
        let claims = claims_for("alice", "user");

        // Mixed-band fixture: grandfathered, public, team-allow (sub match),
        // team-deny (no match, no groups), private-allow (creator),
        // private-deny (other creator), same-band pair (collapse target),
        // status-tagged band variants.
        let fixture = vec![
            // Grandfathered — no auth_context, always included without eval.
            asset("gf-1", "bob", None, vec![]),
            // Public — allow.
            asset(
                "pub-1",
                "bob",
                Some(AuthContext {
                    visibility: "public".to_string(),
                    owner_groups: vec![],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // Team + owner_groups contains principal sub — allow (eda88e15 arm).
            asset(
                "team-allow-1",
                "bob",
                Some(AuthContext {
                    visibility: "team".to_string(),
                    owner_groups: vec!["alice".to_string()],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // Team + owner_groups no match, no groups claim — deny.
            asset(
                "team-deny-1",
                "bob",
                Some(AuthContext {
                    visibility: "team".to_string(),
                    owner_groups: vec!["carol".to_string()],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // Private + creator — allow.
            asset(
                "priv-allow-1",
                "alice",
                Some(AuthContext {
                    visibility: "private".to_string(),
                    owner_groups: vec![],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // Private + other creator — deny.
            asset(
                "priv-deny-1",
                "bob",
                Some(AuthContext {
                    visibility: "private".to_string(),
                    owner_groups: vec![],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // SAME band as team-allow-1 (different id only) — the collapse
            // target: one eval must cover both.
            asset(
                "team-allow-2",
                "bob",
                Some(AuthContext {
                    visibility: "team".to_string(),
                    owner_groups: vec!["alice".to_string()],
                    confidentiality: String::new(),
                }),
                vec![],
            ),
            // Same band as team-allow-1 PLUS a status tag — different band
            // (status participates in the entity), same expected verdict.
            asset(
                "team-allow-3",
                "bob",
                Some(AuthContext {
                    visibility: "team".to_string(),
                    owner_groups: vec!["alice".to_string()],
                    confidentiality: String::new(),
                }),
                vec![tag("status", "draft")],
            ),
        ];

        let per_row = filter_by_permission(&authorizer, &claims, "View", fixture.clone());
        let batched = filter_by_permission_batched(&authorizer, &claims, "View", fixture.clone());

        assert_eq!(
            visible_ids(&per_row),
            visible_ids(&batched),
            "batched filter must produce the IDENTICAL visible set to the per-row filter"
        );

        // Expected verdicts, independently stated:
        let expected = vec![
            "gf-1",         // grandfathered
            "pub-1",        // public
            "team-allow-1", // sub match
            "priv-allow-1", // creator
            "team-allow-2", // same band as team-allow-1
            "team-allow-3", // status-tagged band variant
        ];
        assert_eq!(visible_ids(&batched), expected);
    }

    #[tokio::test]
    async fn batch_collapses_same_band_to_one_eval() {
        // The collapse claim itself: a mailbox-class read (N rows, one band)
        // must yield the same verdict for every row — and the batched filter
        // must include every row the per-row filter includes.
        let authorizer = test_authorizer();
        let claims = claims_for("alice", "user");

        let fixture: Vec<Asset> = (0..50)
            .map(|i| {
                asset(
                    &format!("msg-{}", i),
                    "bob",
                    Some(AuthContext {
                        visibility: "team".to_string(),
                        owner_groups: vec!["alice".to_string()],
                        confidentiality: String::new(),
                    }),
                    vec![],
                )
            })
            .collect();

        let per_row = filter_by_permission(&authorizer, &claims, "View", fixture.clone());
        let batched = filter_by_permission_batched(&authorizer, &claims, "View", fixture.clone());

        assert_eq!(per_row.len(), 50, "sub match must allow all 50 rows");
        assert_eq!(
            visible_ids(&per_row),
            visible_ids(&batched),
            "one-band batch must match the per-row verdict for every row"
        );
    }
}
