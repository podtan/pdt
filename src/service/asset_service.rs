//! Asset service

use chrono::Utc;
use serde_json::json;

use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, Asset, AuditAction, AuthContext, CreateAssetRequest, Tag, UpdateAssetRequest,
    UpdateAuthContextRequest,
};
use crate::service::Services;

/// GUARD (issue 0e583f06, born-masked class): empty visibility+owner_groups
/// must be unrepresentable at birth. An asset stored with visibility=""
/// matches NO View permit except admin/creator — the born-mask class
/// (assignees and cohorts invisible to themselves; 7 manual heals on
/// 2026-10-20 alone). The sqlite INSERT binds "" for absent contexts, so an
/// un-stamped create lands in that black hole instead of the documented
/// grandfathered-open state.
///
/// Default ladder (applied to EVERY create, both repository backends):
/// - owner_groups provided, visibility empty → "team" (the cohort IS the intent)
/// - both empty, caller has groups → "team" + caller's groups (cohort band —
///   the nghr-proven birth stamp shape, now enforced at the PDT layer for
///   every caller)
/// - both empty, caller has NO groups → "private" (honest creator-only; a
///   loud WARN replaces the silent black hole — refusing would break live
///   group-less service callers, e.g. agent svcacct read-state writes)
/// - visibility="team" with empty owner_groups → owner_groups filled from the
///   caller (team-without-cohort grants creator+admin only — a lie); no
///   caller cohort → "private" + WARN.
/// Explicit non-empty visibility ("public"/"org"/"private") is untouched, and
/// caller-provided owner_groups are never overwritten — only filled when empty.
pub fn guard_birth_auth_context(
    requested: Option<AuthContext>,
    caller_groups: &[String],
    user_id: &str,
) -> AuthContext {
    let mut ctx = requested.unwrap_or_default();
    if ctx.visibility.is_empty() {
        if !ctx.owner_groups.is_empty() {
            ctx.visibility = "team".to_string();
        } else if !caller_groups.is_empty() {
            ctx.visibility = "team".to_string();
            ctx.owner_groups = caller_groups.to_vec();
        } else {
            tracing::warn!(
                "BORN-EMPTY GUARD (0e583f06): asset create by {} carries no visibility \
                 and no owner_groups, and the caller has no groups claim — defaulting \
                 to private (creator-only). Provide auth_context to band this asset \
                 for its cohort.",
                user_id
            );
            ctx.visibility = "private".to_string();
        }
    } else if ctx.visibility == "team" && ctx.owner_groups.is_empty() {
        if !caller_groups.is_empty() {
            ctx.owner_groups = caller_groups.to_vec();
        } else {
            tracing::warn!(
                "BORN-EMPTY GUARD (0e583f06): asset create by {} declares \
                 visibility=team with no owner_groups and the caller has no groups \
                 claim — defaulting to private (team without a cohort grants \
                 creator+admin only).",
                user_id
            );
            ctx.visibility = "private".to_string();
        }
    }
    ctx
}

/// Service for asset business logic
pub struct AssetService;

impl AssetService {
    /// Create a new asset with validation and the born-empty guard (0e583f06)
    pub async fn create(
        services: &Services,
        request: CreateAssetRequest,
        user_id: &str,
        caller_groups: &[String],
    ) -> Result<Asset> {
        // Validate request
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }

        // BORN-EMPTY GUARD (0e583f06): empty visibility+owner_groups is
        // unrepresentable — default to the caller's cohort band (or an honest
        // private) before the repository stores the black-hole "" state.
        let guarded_ctx =
            guard_birth_auth_context(request.auth_context.clone(), caller_groups, user_id);
        let mut guarded = request.clone();
        guarded.auth_context = Some(guarded_ctx);

        // Create asset
        let asset = services.assets().create(guarded, user_id).await?;

        // Create audit entry
        services
            .audit()
            .create(
                "asset",
                &asset.id,
                AuditAction::Create,
                json!({
                    "title": asset.title,
                    "tags": asset.tags.len(),
                }),
                user_id,
            )
            .await?;

        Ok(asset)
    }

    /// Get asset by ID
    pub async fn get(services: &Services, id: &str) -> Result<Asset> {
        services.assets().get_by_id(id).await
    }

    /// Update an asset
    pub async fn update(
        services: &Services,
        id: &str,
        request: UpdateAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Validate request
        if let Some(ref title) = request.title {
            if title.trim().is_empty() {
                return Err(ApiError::Validation("Title cannot be empty".to_string()));
            }
        }

        // Get current state for audit
        let old_asset = services.assets().get_by_id(id).await?;

        // Update asset
        let asset = services
            .assets()
            .update(id, request.clone(), user_id)
            .await?;

        // Create audit entry
        services
            .audit()
            .create(
                "asset",
                id,
                AuditAction::Update,
                json!({
                    "old": {
                        "title": old_asset.title,
                        "content": old_asset.content,
                    },
                    "new": {
                        "title": request.title,
                        "content": request.content,
                    }
                }),
                user_id,
            )
            .await?;

        Ok(asset)
    }

    /// Delete an asset (soft delete)
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        // Verify asset exists
        services.assets().get_by_id(id).await?;

        // Delete asset
        services.assets().soft_delete(id).await?;

        // Delete related relations
        services.relations().delete_by_asset(id).await?;

        // Remove from collections
        services.collections().remove_asset_from_all(id).await?;

        // Create audit entry
        services
            .audit()
            .create("asset", id, AuditAction::Delete, json!({}), user_id)
            .await?;

        Ok(())
    }

    /// List assets with pagination
    /// Filter by asset type tag if specified
    pub async fn list(
        services: &Services,
        limit: i64,
        cursor: Option<&str>,
        asset_type_tag: Option<&str>,
        sort_by: &str,
        order: &str,
    ) -> Result<(Vec<Asset>, Option<String>)> {
        services
            .assets()
            .list(limit, cursor, asset_type_tag, sort_by, order)
            .await
    }

    /// Add a tag to an asset
    pub async fn add_tag(
        services: &Services,
        asset_id: &str,
        request: AddTagRequest,
        user_id: &str,
    ) -> Result<Tag> {
        // Validate the tag request
        request.validate().map_err(ApiError::Validation)?;

        let tag = services
            .assets()
            .add_tag(asset_id, request.clone(), user_id)
            .await?;

        // Create audit entry
        services
            .audit()
            .create(
                "asset",
                asset_id,
                AuditAction::AddTag,
                json!({
                    "tag_id": tag.id,
                    "category": &tag.category,
                    "value": &tag.value,
                }),
                user_id,
            )
            .await?;

        Ok(tag)
    }

    /// Remove a tag from an asset
    pub async fn remove_tag(
        services: &Services,
        asset_id: &str,
        tag_id: &str,
        user_id: &str,
    ) -> Result<()> {
        services.assets().remove_tag(asset_id, tag_id).await?;

        // Create audit entry
        services
            .audit()
            .create(
                "asset",
                asset_id,
                AuditAction::RemoveTag,
                json!({ "tag_id": tag_id }),
                user_id,
            )
            .await?;

        Ok(())
    }

    /// Update the authorization context of an asset
    pub async fn update_auth_context(
        services: &Services,
        id: &str,
        request: UpdateAuthContextRequest,
        user_id: &str,
    ) -> Result<Asset> {
        // Get current asset
        let asset = services.assets().get_by_id(id).await?;

        // Merge request fields into existing auth_context (or create default)
        let mut ctx = asset.auth_context.unwrap_or_default();
        if let Some(ref visibility) = request.visibility {
            ctx.visibility = visibility.clone();
        }
        if let Some(ref owner_groups) = request.owner_groups {
            ctx.owner_groups = owner_groups.clone();
        }
        if let Some(ref confidentiality) = request.confidentiality {
            ctx.confidentiality = confidentiality.clone();
        }

        let updated = services.assets().update_auth_context(id, &ctx).await?;

        // Cascade to descendants via relation graph if requested
        let cascaded_count = if request.cascade {
            let descendants = services.relations().get_descendants(id).await?;
            let count = descendants.len();
            for child_id in &descendants {
                if let Ok(child) = services.assets().get_by_id(child_id).await {
                    // Merge: only update fields that were explicitly set in the request,
                    // preserving child-specific overrides for unset fields
                    let mut child_ctx = child.auth_context.unwrap_or_default();
                    if request.visibility.is_some() {
                        child_ctx.visibility = ctx.visibility.clone();
                    }
                    if request.owner_groups.is_some() {
                        child_ctx.owner_groups = ctx.owner_groups.clone();
                    }
                    if request.confidentiality.is_some() {
                        child_ctx.confidentiality = ctx.confidentiality.clone();
                    }
                    let _ = services
                        .assets()
                        .update_auth_context(child_id, &child_ctx)
                        .await;
                }
            }
            count
        } else {
            0
        };

        // Audit
        services
            .audit()
            .create(
                "asset",
                id,
                AuditAction::Update,
                json!({
                    "action": "update_auth_context",
                    "auth_context": &ctx,
                    "cascade": request.cascade,
                    "cascaded_count": cascaded_count,
                }),
                user_id,
            )
            .await?;

        Ok(updated)
    }

    /// Retention prune candidates (CPO ruling 8d591e85, retention policy
    /// b78efcd): assets carrying a tag (category, value) whose created_at is
    /// older than the window. The policy LIVES IN DATA (the retention asset
    /// supplies category/value/window at invocation time) — this code encodes
    /// only the mechanics; nothing here may hardcode a policy window or class
    /// (data must not leak into code).
    ///
    /// Cursor-walks the tag-filtered search to a hard cap (runaway-walk
    /// guard) and reports truncation honestly.
    pub async fn prune_candidates(
        services: &Services,
        tag_category: &str,
        tag_value: &str,
        older_than_days: u32,
    ) -> Result<Vec<Asset>> {
        const MAX_CANDIDATE_WALK: usize = 10_000;
        let cutoff = Utc::now() - chrono::Duration::days(older_than_days as i64);
        let mut candidates = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let (page, next) = services
                .assets()
                .search(
                    None,
                    vec![(tag_category.to_string(), tag_value.to_string())],
                    500,
                    cursor.as_deref(),
                )
                .await?;
            for asset in page {
                if asset.created_at < cutoff {
                    candidates.push(asset);
                    if candidates.len() >= MAX_CANDIDATE_WALK {
                        return Ok(candidates);
                    }
                }
            }
            match next {
                Some(c) => cursor = Some(c),
                None => return Ok(candidates),
            }
        }
    }

    /// Retention prune (CPO ruling 8d591e85 — BINDING GATES, all four):
    /// 1. ARCHIVE-OVER-DELETE hard line: removal is soft-delete ONLY
    ///    (deleted_at stamp — recoverable at the store); never a hard delete.
    /// 2. EXECUTION SOFT-DELETE-ONLY: every removal rides the full
    ///    AssetService::delete path (soft_delete + relation cascade +
    ///    collections cleanup + ONE audit entry) — no bare row kills.
    /// 3. FIRST EXECUTION gated on the CPO's sight of the dry-run numbers:
    ///    the WIRE gate is the handler's PDT_PRUNE_ENABLED check — this
    ///    function refuses dry_run=false unless the caller already cleared
    ///    it (defense-in-depth behind the handler gate).
    /// 4. Scheduled+announced windows with pre-prune snapshots: procedural —
    ///    the dry-run report exists to be sighted and announced.
    pub async fn prune(
        services: &Services,
        tag_category: &str,
        tag_value: &str,
        older_than_days: u32,
        dry_run: bool,
        user_id: &str,
    ) -> Result<serde_json::Value> {
        let candidates =
            Self::prune_candidates(services, tag_category, tag_value, older_than_days).await?;
        let matched = candidates.len();

        if dry_run {
            // Dry-run: the numbers only. Zero writes, zero audits.
            let listed: Vec<serde_json::Value> = candidates
                .iter()
                .take(100)
                .map(|a| {
                    json!({
                        "id": a.id,
                        "title": a.title,
                        "created_at": a.created_at.to_rfc3339(),
                    })
                })
                .collect();
            return Ok(json!({
                "dry_run": true,
                "matched": matched,
                "filter": {"tag_category": tag_category, "tag_value": tag_value},
                "older_than_days": older_than_days,
                "candidates": listed,
            }));
        }

        // Execute: soft-delete-only, full cascade + audit per asset.
        let mut pruned = 0u64;
        let mut failed: Vec<String> = Vec::new();
        for asset in &candidates {
            match Self::delete(services, &asset.id, user_id).await {
                Ok(()) => pruned += 1,
                Err(_) => failed.push(asset.id.clone()),
            }
        }
        Ok(json!({
            "dry_run": false,
            "matched": matched,
            "pruned": pruned,
            "failed": failed,
            "filter": {"tag_category": tag_category, "tag_value": tag_value},
            "older_than_days": older_than_days,
        }))
    }
}

#[cfg(test)]
mod born_empty_guard_tests {
    use super::*;

    fn groups(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn born_empty_with_caller_groups_defaults_to_cohort_band() {
        // auth_context: None + caller with groups → team + caller's groups
        let ctx = guard_birth_auth_context(None, &groups(&["pdt_admins@idp.tanbal.ir"]), "svc");
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["pdt_admins@idp.tanbal.ir"]));
    }

    #[test]
    fn born_empty_without_caller_groups_defaults_to_private() {
        // auth_context: None + group-less caller → private (honest creator-only)
        let ctx = guard_birth_auth_context(None, &[], "svc");
        assert_eq!(ctx.visibility, "private");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn empty_visibility_with_owner_groups_normalizes_to_team() {
        // visibility="" + owner_groups provided → team, owner_groups PRESERVED
        let requested = AuthContext {
            visibility: String::new(),
            owner_groups: groups(&["cohort-a"]),
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &groups(&["pdt_admins"]), "svc");
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["cohort-a"]));
    }

    #[test]
    fn team_without_owner_groups_fills_from_caller() {
        // visibility="team" + empty owner_groups + caller groups → filled
        let requested = AuthContext {
            visibility: "team".to_string(),
            owner_groups: vec![],
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &groups(&["devs"]), "svc");
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["devs"]));
    }

    #[test]
    fn team_without_owner_groups_and_no_caller_groups_downgrades_to_private() {
        let requested = AuthContext {
            visibility: "team".to_string(),
            owner_groups: vec![],
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &[], "svc");
        assert_eq!(ctx.visibility, "private");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn explicit_public_is_untouched() {
        let requested = AuthContext {
            visibility: "public".to_string(),
            owner_groups: vec![],
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &groups(&["devs"]), "svc");
        assert_eq!(ctx.visibility, "public");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn explicit_private_is_untouched() {
        let requested = AuthContext {
            visibility: "private".to_string(),
            owner_groups: vec![],
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &groups(&["devs"]), "svc");
        assert_eq!(ctx.visibility, "private");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn explicit_org_is_untouched() {
        let requested = AuthContext {
            visibility: "org".to_string(),
            owner_groups: vec![],
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &[], "svc");
        assert_eq!(ctx.visibility, "org");
    }

    #[test]
    fn caller_owner_groups_are_never_overwritten() {
        // caller provided owner_groups + empty visibility → team + THEIR groups
        let requested = AuthContext {
            visibility: String::new(),
            owner_groups: groups(&["assignees@idp.tanbal.ir"]),
            confidentiality: String::new(),
        };
        let ctx = guard_birth_auth_context(Some(requested), &groups(&["caller-group"]), "svc");
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["assignees@idp.tanbal.ir"]));
    }

    #[test]
    fn dev_mode_style_groups_produce_cohort_band() {
        // dev_mode injects groups=["developers"] — dev creates get a real band
        let ctx = guard_birth_auth_context(None, &groups(&["developers"]), "dev-user-id");
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["developers"]));
    }
}
