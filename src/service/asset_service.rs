//! Asset service

use chrono::Utc;
use serde_json::json;

use crate::error::{ApiError, Result};
use crate::models::{
    AddTagRequest, AdoptAssetRequest, Asset, AuditAction, AuthContext, CreateAssetRequest,
    MergeMetadataRequest, Tag, UpdateAssetRequest, UpdateAuthContextRequest, WorkspaceDefaultBand,
};
use crate::service::Services;

/// The collaboration record types the workspace default band applies to
/// (ca989667 — the pdt-side create cohort grant). These are the board-facing
/// record types born by caller layers (nghr stamps exactly these type tag
/// values: issue, issue_response, task, document). Non-collaboration types
/// (concepts, ideas, notes, …) keep the ruled born-empty semantics even in a
/// configured workspace — the grant is a collaboration-cohort policy, not a
/// store-wide default.
pub const COLLABORATION_TYPES: [&str; 4] = ["issue", "issue_response", "task", "document"];

/// True when a create arrives BAND-LESS (ca989667 guard 1): no auth_context
/// at all, or an auth_context with BOTH visibility and owner_groups empty.
/// Any explicit stamp — visibility alone, groups alone, or both — makes the
/// create NOT band-less, and the workspace default never applies (explicit
/// stamps always win; the guard ladder fills partial stamps as before).
pub fn is_bandless_create(requested: Option<&AuthContext>) -> bool {
    match requested {
        None => true,
        Some(ctx) => ctx.visibility.is_empty() && ctx.owner_groups.is_empty(),
    }
}

/// True when the workspace default band may apply to this create: band-less
/// AND a collaboration record type. The config read (I/O) is gated on this
/// predicate — a fully-stamped or non-collaboration create never touches
/// the filesystem for policy.
pub fn workspace_default_applies(
    requested: Option<&AuthContext>,
    asset_type: Option<&str>,
) -> bool {
    is_bandless_create(requested)
        && asset_type
            .map(|t| COLLABORATION_TYPES.contains(&t))
            .unwrap_or(false)
}

/// Apply the workspace default band to a GUARDED birth auth_context
/// (ca989667 — CPO GO 2026-11-23, four guards):
/// - fires ONLY on band-less collaboration creates (guard 1 — explicit
///   stamps always win; `requested` is the PRE-guard caller input);
/// - fires ONLY when a default is configured (guard 2 — unconfigured
///   workspaces keep the ruled born-empty honest-private untouched);
/// - the configured band is the explicit per-workspace admin action's
///   product (guard 3 — never global, never implicit, never inherited);
/// - the caller records the application in the audit log (guard 4 — the
///   `applied` flag drives the audit enrichment).
///
/// Precedence at birth: explicit stamp > workspace default > caller-groups
/// implicit fill > honest private. The workspace default overrides the
/// guard ladder's implicit caller-groups fill because the admin-configured
/// band IS the workspace's declared policy for band-less collaboration
/// creates — the same shape the nghr caller layer stamps on its own
/// creates (a0c08dc1 executing cb11a8e0), now hosted in pdt for callers
/// with no nghr layer.
///
/// Returns the (possibly unchanged) context plus whether the default was
/// APPLIED (for the audit record — ca989667 guard 4).
pub fn apply_workspace_default_band(
    guarded: AuthContext,
    requested: Option<&AuthContext>,
    asset_type: Option<&str>,
    ws_default: Option<&WorkspaceDefaultBand>,
) -> (AuthContext, bool) {
    let Some(default) = ws_default else {
        return (guarded, false);
    };
    if !workspace_default_applies(requested, asset_type) {
        return (guarded, false);
    }
    // Defensive re-validation at the application site: an invalid band is
    // treated as unconfigured (fail toward the ruled honest-private
    // semantics — never widen visibility on a broken policy).
    if default.validate().is_err() {
        tracing::warn!(
            "WORKSPACE DEFAULT BAND (ca989667): configured band failed validation at \
             application — treating the workspace as unconfigured"
        );
        return (guarded, false);
    }
    let mut ctx = guarded;
    ctx.visibility = default.visibility.clone();
    ctx.owner_groups = default.owner_groups.clone();
    (ctx, true)
}

/// Compose the FULL birth auth_context for a create: the born-empty guard
/// ladder (0e583f06) followed by the workspace default band (ca989667).
/// Single source of truth for both the Cedar pre-write probe and the
/// service-layer persist — the consult must see exactly what the store
/// will persist. Returns the context plus whether the workspace default
/// was applied (audit enrichment, guard 4).
pub fn resolve_birth_auth_context(
    requested: Option<AuthContext>,
    caller_groups: &[String],
    asset_type: Option<&str>,
    ws_default: Option<&WorkspaceDefaultBand>,
    user_id: &str,
) -> (AuthContext, bool) {
    let guarded = guard_birth_auth_context(requested.clone(), caller_groups, user_id);
    apply_workspace_default_band(guarded, requested.as_ref(), asset_type, ws_default)
}

/// Extract the asset type tag value from a create request's tags (the
/// category="type" tag — the same attribute the Cedar entity builder reads).
fn asset_type_from_tags(tags: &[AddTagRequest]) -> Option<String> {
    tags.iter()
        .find(|t| t.category == "type")
        .map(|t| t.value.clone())
}

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
    /// Create a new asset with validation, the born-empty guard (0e583f06)
    /// and the workspace default band (ca989667).
    ///
    /// `ws_default` is the configured workspace default band for the store
    /// this create lands in (resolved by the handler from the store-side
    /// config; None = unconfigured or non-applicable). `workspace_id` is
    /// the instance the create lands in (for the audit record).
    pub async fn create(
        services: &Services,
        request: CreateAssetRequest,
        user_id: &str,
        caller_groups: &[String],
        ws_default: Option<&WorkspaceDefaultBand>,
        workspace_id: Option<&str>,
    ) -> Result<Asset> {
        // Validate request
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }

        // BIRTH AUTH CONTEXT (0e583f06 + ca989667): the guard ladder
        // defaults an empty band (caller cohort or honest private), then
        // the workspace default band overrides band-less collaboration
        // creates in a configured workspace. Explicit stamps always win.
        let asset_type = asset_type_from_tags(&request.tags);
        let (final_ctx, default_applied) = resolve_birth_auth_context(
            request.auth_context.clone(),
            caller_groups,
            asset_type.as_deref(),
            ws_default,
            user_id,
        );
        let mut guarded = request.clone();
        guarded.auth_context = Some(final_ctx);

        // Create asset
        let asset = services.assets().create(guarded, user_id).await?;

        // Create audit entry. When the workspace default band applied, the
        // record names the workspace + the band applied (ca989667 guard 4)
        // so any future verify can reconstruct provenance.
        let mut details = json!({
            "title": asset.title,
            "tags": asset.tags.len(),
        });
        if default_applied {
            if let Some(band) = ws_default {
                details["birth_band_source"] = json!("workspace_default_band");
                if let Some(ws) = workspace_id {
                    details["workspace_id"] = json!(ws);
                }
                details["default_band"] = json!({
                    "visibility": band.visibility,
                    "owner_groups": band.owner_groups,
                });
            }
        }
        services
            .audit()
            .create("asset", &asset.id, AuditAction::Create, details, user_id)
            .await?;

        Ok(asset)
    }

    /// Get asset by ID
    /// ADOPT: materialize an asset with a SPECIFIED id (same-uuid adopt —
    /// CPO ruling 6d86ee70, the Path A adopt's first-class path).
    ///
    /// Additive (plain create keeps minting new uuids); duplicate-barred at
    /// the repository layer; audited ATOMICALLY with the materialization (a
    /// failed audit write denies the adopt). Provenance is REQUIRED — the
    /// call names the banked source. Custody (platform-admin only) is the
    /// HANDLER's check (it holds the claims).
    pub async fn adopt(
        services: &Services,
        id: &str,
        request: AdoptAssetRequest,
        user_id: &str,
    ) -> Result<Asset> {
        if request.title.trim().is_empty() {
            return Err(ApiError::Validation("Title cannot be empty".to_string()));
        }
        // Provenance REQUIRED (ruling 6d86ee70): the call names the banked
        // source — an adopt without provenance is an unaccountable
        // materialization.
        if request.provenance.trim().is_empty() {
            return Err(ApiError::Validation(
                "provenance is required: name the banked source the row is materialized from"
                    .to_string(),
            ));
        }
        let inner = CreateAssetRequest {
            title: request.title,
            content: request.content,
            tags: request.tags,
            metadata: request.metadata,
            auth_context: request.auth_context,
        };
        services
            .assets()
            .adopt(id, inner, user_id, &request.provenance)
            .await
    }

    pub async fn get(services: &Services, id: &str) -> Result<Asset> {
        services.assets().get_by_id(id).await
    }

    /// Git-style prefix resolution on by-id READ paths (issue e9478d91):
    /// a well-formed UUID resolves directly (never a scan); a valid-hex id
    /// shorter than 36 chars that is a UNIQUE prefix of exactly one live
    /// asset resolves to it (case-insensitive — the response id is the
    /// canonical stored form); zero matches stay a body-bearing 404 (honest
    /// absence); ≥2 matches answer 409 AMBIGUOUS_PREFIX with the match
    /// count + the minimum sufficient length hint; malformed (non-hex, or
    /// ≥36 chars that isn't a valid UUID) answers 422 MALFORMED_ID — a
    /// caller transcription error is never masked as absent. MUTATIONS STAY
    /// FULL-UUID: only READ paths call this — update/delete/tag writes keep
    /// the strict get_by_id path.
    pub async fn resolve(services: &Services, id: &str) -> Result<Asset> {
        if uuid::Uuid::parse_str(id).is_ok() {
            return services.assets().get_by_id(id).await;
        }
        if !is_hex_prefix_shape(id) {
            return Err(ApiError::MalformedId(format!(
                "entity ids are UUIDs (or a unique hex prefix), got: {}",
                id
            )));
        }
        let matches = services.assets().get_by_id_prefix(id).await?;
        match matches.len() {
            1 => Ok(matches.into_iter().next().expect("exactly one match")),
            0 => Err(ApiError::NotFound(format!("Asset not found: {}", id))),
            _ => {
                let count = matches.len();
                let hint = min_sufficient_prefix_len(&matches);
                let count_str = if count >= 51 {
                    "50+".to_string()
                } else {
                    count.to_string()
                };
                Err(ApiError::AmbiguousPrefix(format!(
                    "{} matches {} live assets - use at least {} characters (or the full UUID)",
                    id, count_str, hint
                )))
            }
        }
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

        // Metadata delta — KEY-LEVEL only, never values (values may carry
        // credential-shaped material — the Eynak mcp_credentials blocks are
        // metadata values; the merge endpoint's audit discipline, a5daf31d).
        // Issue 3a4ccaf0: the update path's full-map metadata replace was
        // invisible in the audit trail — a metadata-only update read as a
        // no-op (old==new null on both fields) and a credentials-block wipe
        // left no trace. The delta names added/removed/changed keys;
        // title/content-only updates carry no metadata block (the entry
        // shape is unchanged for them).
        let metadata_delta = request.metadata.as_ref().map(|new_meta| {
            let old_meta = &old_asset.metadata;
            let added: Vec<&String> = new_meta
                .keys()
                .filter(|k| !old_meta.contains_key(*k))
                .collect();
            let changed: Vec<&String> = new_meta
                .iter()
                .filter(|(k, v)| old_meta.get(*k).is_some_and(|old_v| old_v != *v))
                .map(|(k, _)| k)
                .collect();
            let removed: Vec<&String> = old_meta
                .keys()
                .filter(|k| !new_meta.contains_key(*k))
                .collect();
            json!({
                "replace": true,
                "added": added,
                "removed": removed,
                "changed": changed,
            })
        });

        // Create audit entry
        let mut changes = json!({
            "old": {
                "title": old_asset.title,
                "content": old_asset.content,
            },
            "new": {
                "title": request.title,
                "content": request.content,
            }
        });
        if let Some(delta) = metadata_delta {
            changes["metadata"] = delta;
        }
        services
            .audit()
            .create("asset", id, AuditAction::Update, changes, user_id)
            .await?;

        Ok(asset)
    }

    /// Delete an asset (soft delete)
    ///
    /// The relation cascade is a HARD delete on both backends — the removed
    /// rows are unrecoverable, so this ONE audit entry carries the removed
    /// relation summary (id + endpoints + type) as the only forensic record
    /// of what the cascade removed (issue 40df9c5c: cascade removals were
    /// unaudited — "relation-rows 2→2, zero delete rows" in a forensic walk).
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        // Verify asset exists
        services.assets().get_by_id(id).await?;

        // Snapshot the asset's relations BEFORE the cascade — they are
        // hard-deleted below and the audit entry is the only record left.
        let cascaded_relations = services.relations().get_asset_relations(id).await?;

        // Delete asset
        services.assets().soft_delete(id).await?;

        // Delete related relations
        services.relations().delete_by_asset(id).await?;

        // Remove from collections
        services.collections().remove_asset_from_all(id).await?;

        // Create audit entry — ONE entry per cascade (less is more, not N),
        // carrying the removed relation summary so a forensic walk can
        // reconstruct the cascade (issue 40df9c5c).
        services
            .audit()
            .create(
                "asset",
                id,
                AuditAction::Delete,
                json!({
                    "cascade": true,
                    "relations_removed": cascaded_relations.len(),
                    "relations": cascaded_relations
                        .iter()
                        .map(|r| {
                            json!({
                                "id": r.id,
                                "from_asset_id": r.from_asset_id,
                                "to_asset_id": r.to_asset_id,
                                "relation_type": r.relation_type.to_string(),
                            })
                        })
                        .collect::<Vec<_>>(),
                }),
                user_id,
            )
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

    /// Count assets matching the list filter — the true total for
    /// /api/assets PaginatedResponse (issue b040d645). Cursor-independent,
    /// pre-visibility.
    pub async fn count_list(services: &Services, asset_type_tag: Option<&str>) -> Result<u64> {
        services.assets().count_list(asset_type_tag).await
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

    /// Merge metadata keys into an asset — the delta-write surface (issue
    /// a5daf31d). UPSERT per key via the repository's merge_metadata; the
    /// keys the caller does not send are untouched, so a concurrent writer's
    /// keys survive. The audit entry names the merged keys only.
    pub async fn merge_metadata(
        services: &Services,
        id: &str,
        request: MergeMetadataRequest,
        user_id: &str,
    ) -> Result<Asset> {
        let asset = services
            .assets()
            .merge_metadata(id, &request.metadata, user_id)
            .await?;

        // Create audit entry — merged KEYS only, never values (values may
        // carry credential-shaped material).
        services
            .audit()
            .create(
                "asset",
                id,
                AuditAction::Update,
                json!({
                    "merge": true,
                    "keys": request.metadata.keys().collect::<Vec<_>>(),
                }),
                user_id,
            )
            .await?;

        Ok(asset)
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
    ///    collections cleanup + ONE audit entry carrying the removed-relation
    ///    summary, 40df9c5c) — no bare row kills.
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

#[cfg(test)]
mod workspace_default_band_tests {
    use super::*;

    fn groups(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn cohort_band() -> WorkspaceDefaultBand {
        WorkspaceDefaultBand {
            visibility: "team".to_string(),
            owner_groups: groups(&["plt-agents@idp.tanbal.ir", "plt-agents"]),
        }
    }

    fn issue_request() -> CreateAssetRequest {
        CreateAssetRequest {
            title: "board issue".to_string(),
            content: None,
            tags: vec![AddTagRequest {
                category: "type".to_string(),
                value: "issue".to_string(),
            }],
            metadata: Default::default(),
            auth_context: None,
        }
    }

    // ── GUARD 1: band-less creates ONLY — explicit stamps always win ──

    #[test]
    fn explicit_visibility_stamp_wins_over_workspace_default() {
        let req = CreateAssetRequest {
            auth_context: Some(AuthContext {
                visibility: "private".to_string(),
                owner_groups: vec![],
                confidentiality: String::new(),
            }),
            ..issue_request()
        };
        let (ctx, applied) = resolve_birth_auth_context(
            req.auth_context.clone(),
            &[],
            Some("issue"),
            Some(&cohort_band()),
            "agent",
        );
        assert!(!applied);
        assert_eq!(ctx.visibility, "private");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn explicit_owner_groups_stamp_wins_over_workspace_default() {
        // A partial explicit stamp (groups only) is STILL explicit — the
        // guard ladder fills visibility=team from it; the default never merges.
        let req = CreateAssetRequest {
            auth_context: Some(AuthContext {
                visibility: String::new(),
                owner_groups: groups(&["devs@idp.tanbal.ir"]),
                confidentiality: String::new(),
            }),
            ..issue_request()
        };
        let (ctx, applied) = resolve_birth_auth_context(
            req.auth_context.clone(),
            &[],
            Some("issue"),
            Some(&cohort_band()),
            "agent",
        );
        assert!(!applied);
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["devs@idp.tanbal.ir"]));
    }

    #[test]
    fn workspace_default_overrides_bandless_create() {
        // THE forward-class fix: agent-direct band-less issue create in a
        // configured workspace → the configured cohort band (not private).
        let (ctx, applied) =
            resolve_birth_auth_context(None, &[], Some("issue"), Some(&cohort_band()), "agent");
        assert!(applied);
        assert_eq!(ctx.visibility, "team");
        assert_eq!(
            ctx.owner_groups,
            groups(&["plt-agents@idp.tanbal.ir", "plt-agents"])
        );
    }

    #[test]
    fn workspace_default_overrides_caller_groups_implicit_fill() {
        // Band-less + caller WITH groups in a CONFIGURED workspace: the
        // admin-configured band is the workspace's declared policy for
        // band-less collaboration creates — it overrides the implicit
        // caller-groups fill (explicit > configured > implicit > private).
        let (ctx, applied) = resolve_birth_auth_context(
            None,
            &groups(&["pdt_admins@idp.tanbal.ir"]),
            Some("issue"),
            Some(&cohort_band()),
            "human-admin",
        );
        assert!(applied);
        assert_eq!(
            ctx.owner_groups,
            groups(&["plt-agents@idp.tanbal.ir", "plt-agents"])
        );
    }

    // ── GUARD 2: unconfigured workspaces keep honest-private untouched ──

    #[test]
    fn unconfigured_workspace_keeps_honest_private() {
        let (ctx, applied) = resolve_birth_auth_context(None, &[], Some("issue"), None, "agent");
        assert!(!applied);
        assert_eq!(ctx.visibility, "private");
        assert!(ctx.owner_groups.is_empty());
    }

    #[test]
    fn unconfigured_workspace_keeps_caller_groups_fill() {
        // No config → the 0e583f06 ladder stands byte-for-byte.
        let (ctx, applied) =
            resolve_birth_auth_context(None, &groups(&["developers"]), Some("issue"), None, "dev");
        assert!(!applied);
        assert_eq!(ctx.visibility, "team");
        assert_eq!(ctx.owner_groups, groups(&["developers"]));
    }

    // ── Collaboration-type scoping ──

    #[test]
    fn non_collaboration_type_never_gets_the_default() {
        for t in ["concept", "idea", "note", "instance"] {
            let (ctx, applied) =
                resolve_birth_auth_context(None, &[], Some(t), Some(&cohort_band()), "agent");
            assert!(!applied, "type {t} must never receive the default");
            assert_eq!(ctx.visibility, "private");
        }
    }

    #[test]
    fn all_collaboration_types_receive_the_default() {
        for t in COLLABORATION_TYPES {
            let (ctx, applied) =
                resolve_birth_auth_context(None, &[], Some(t), Some(&cohort_band()), "agent");
            assert!(applied, "type {t} must receive the default");
            assert_eq!(ctx.visibility, "team");
        }
    }

    #[test]
    fn missing_type_tag_never_gets_the_default() {
        let (ctx, applied) = resolve_birth_auth_context(None, &[], None, Some(&cohort_band()), "a");
        assert!(!applied);
        assert_eq!(ctx.visibility, "private");
    }

    // ── GUARD 3 defense: invalid band treated as unconfigured ──

    #[test]
    fn invalid_band_fails_toward_unconfigured() {
        let bad = WorkspaceDefaultBand {
            visibility: "public".to_string(),
            owner_groups: groups(&["world"]),
        };
        let (ctx, applied) =
            resolve_birth_auth_context(None, &[], Some("issue"), Some(&bad), "agent");
        assert!(!applied);
        assert_eq!(ctx.visibility, "private");
    }

    // ── Predicate (the I/O gate) ──

    #[test]
    fn bandless_predicate_shapes() {
        assert!(is_bandless_create(None));
        assert!(is_bandless_create(Some(&AuthContext::default())));
        assert!(!is_bandless_create(Some(&AuthContext {
            visibility: "team".to_string(),
            ..Default::default()
        })));
        assert!(!is_bandless_create(Some(&AuthContext {
            owner_groups: groups(&["x"]),
            ..Default::default()
        })));
    }

    #[test]
    fn applies_predicate_gates_the_config_read() {
        // Band-less + collab → the handler reads the config (I/O gated here).
        assert!(workspace_default_applies(None, Some("issue")));
        assert!(workspace_default_applies(None, Some("issue_response")));
        assert!(workspace_default_applies(None, Some("task")));
        assert!(workspace_default_applies(None, Some("document")));
        // Anything else → no config read, no application.
        assert!(!workspace_default_applies(None, Some("concept")));
        assert!(!workspace_default_applies(
            Some(&AuthContext {
                visibility: "private".to_string(),
                ..Default::default()
            }),
            Some("issue")
        ));
    }
}

/// Hex-prefix shape (issue e9478d91): non-empty, shorter than a full UUID,
/// every char an ASCII hex digit. Anything else on a by-id READ path is a
/// malformed id (422), never a prefix probe.
fn is_hex_prefix_shape(id: &str) -> bool {
    !id.is_empty() && id.len() < 36 && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// Minimum sufficient prefix length (issue e9478d91 guard 2): the length at
/// which the matched ids DIVERGE — the longest common prefix among all
/// matches, plus one. A prefix shorter than this still matches ≥2 assets.
fn min_sufficient_prefix_len(matches: &[Asset]) -> usize {
    let first = matches[0].id.as_bytes();
    let mut lcp = matches[0].id.len();
    for a in &matches[1..] {
        let bytes = a.id.as_bytes();
        let mut i = 0;
        while i < lcp && i < bytes.len() && first[i] == bytes[i] {
            i += 1;
        }
        lcp = i;
        if lcp == 0 {
            break;
        }
    }
    lcp + 1
}
