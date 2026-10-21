//! Roles service — sub-keyed role resolution + idempotent backfill (1552c098).
//!
//! Resolution (one lookup, both families — Bonyan rider point 1):
//! 1. Find the principal asset by `sub` tag; fallback: agent assets keyed by
//!    asset id == sub (agent service accounts).
//! 2. Global role from the `global_role` tag (default user, least privilege).
//! 3. Membership rows from outgoing relations: `has_instance` = owner (the
//!    ownership truth tocpi already keys on), `has_access` metadata role
//!    (legacy "admin" → ws_admin; missing/unknown → member).

use std::collections::HashSet;

use crate::error::{ApiError, Result};
use crate::models::roles::{
    normalize_membership_role, MembershipRoleRow, RoleAnswer, DEFAULT_GLOBAL_ROLE,
    GLOBAL_ROLE_AGENT, GLOBAL_ROLE_USER, MEMBERSHIP_ROLE_OWNER,
};
use crate::models::{AddTagRequest, CreateRelationRequest, RelationType};
use crate::service::Services;

/// Page size for backfill scans.
const BACKFILL_PAGE: i64 = 200;

/// Roles service — sub-keyed resolution + idempotent backfill (1552c098).
pub struct RolesService;

impl RolesService {
    /// Resolve the role answer for a sub — ONE lookup returning BOTH families
    /// (Bonyan rider point 1, spec of record 2fea489c).
    ///
    /// Wire contract (rider point 2): a sub with zero roles — known or unknown —
    /// answers 200 with least-privilege defaults (authorize-nothing deny); only
    /// transport/5xx is a hard error. No time-of-check data in the answer
    /// (rider point 4).
    pub async fn resolve(services: &Services, sub: &str) -> Result<RoleAnswer> {
        if sub.is_empty() {
            return Err(ApiError::BadRequest("sub must be non-empty".to_string()));
        }

        let principal = find_principal(services, sub).await?;

        let (global_role, membership) = match principal {
            // Unknown sub → authorize-nothing at the least-privilege defaults.
            // Still 200 (valid deny), never 404/5xx — rider point 2.
            None => (DEFAULT_GLOBAL_ROLE.to_string(), Vec::new()),
            Some(asset) => {
                // Global role: the asset's global_role tag, default user.
                let global_role = asset
                    .tags
                    .iter()
                    .find(|t| t.category == "global_role")
                    .map(|t| t.value.clone())
                    .unwrap_or_else(|| DEFAULT_GLOBAL_ROLE.to_string());

                // Membership rows: outgoing has_instance (owner) + has_access.
                let relations = services.relations().get_asset_relations(&asset.id).await?;
                let mut membership: Vec<MembershipRoleRow> = Vec::new();
                let mut claimed: HashSet<String> = HashSet::new();

                // has_instance = the ownership truth → owner row.
                for rel in relations.iter().filter(|r| {
                    r.from_asset_id == asset.id && r.relation_type == RelationType::HasInstance
                }) {
                    claimed.insert(rel.to_asset_id.clone());
                    membership.push(MembershipRoleRow {
                        workspace_id: rel.to_asset_id.clone(),
                        role: MEMBERSHIP_ROLE_OWNER.to_string(),
                    });
                }

                // has_access rows — role from metadata (normalized), default member.
                for rel in relations.iter().filter(|r| {
                    r.from_asset_id == asset.id && r.relation_type == RelationType::HasAccess
                }) {
                    // has_instance already claimed this workspace as owner.
                    if claimed.insert(rel.to_asset_id.clone()) {
                        membership.push(MembershipRoleRow {
                            workspace_id: rel.to_asset_id.clone(),
                            role: normalize_membership_role(
                                rel.metadata.get("role").and_then(|v| v.as_str()),
                            )
                            .to_string(),
                        });
                    }
                }

                (global_role, membership)
            }
        };

        Ok(RoleAnswer {
            sub: sub.to_string(),
            global_role,
            membership,
        })
    }

    /// Backfill known roles from live state (1552c098) — IDEMPOTENT.
    ///
    /// 1. Materialize owner membership rows: every `has_instance` (User →
    ///    Workspace) pair without a `has_access` row gains one with role=owner.
    /// 2. Agent assets (type=agent) gain `global_role=agent` when absent.
    /// 3. User assets (type=user) gain `global_role=user` when absent.
    ///
    /// Re-runs clean: every step skips work already done. Returns a summary.
    pub async fn backfill(services: &Services, user_id: &str) -> Result<serde_json::Value> {
        let owners_materialized = backfill_owners(services, user_id).await?;
        let agents_tagged =
            backfill_global_role(services, user_id, "agent", GLOBAL_ROLE_AGENT).await?;
        let users_tagged =
            backfill_global_role(services, user_id, "user", GLOBAL_ROLE_USER).await?;

        let summary: serde_json::Value = serde_json::json!({
            "owners_materialized": owners_materialized,
            "agents_tagged": agents_tagged,
            "users_tagged": users_tagged,
        });
        Ok(summary)
    }
}

/// Find the principal asset for a sub: `sub` tag first, then asset id == sub
/// (agent service accounts key their registry row by the token sub).
/// DB errors propagate honestly — only NotFound means absent.
async fn find_principal(services: &Services, sub: &str) -> Result<Option<crate::models::Asset>> {
    let (assets, _) = services
        .assets()
        .search(None, vec![("sub".to_string(), sub.to_string())], 1, None)
        .await?;
    if let Some(asset) = assets.into_iter().next() {
        return Ok(Some(asset));
    }
    match services.assets().get_by_id(sub).await {
        Ok(asset) => Ok(Some(asset)),
        Err(ApiError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Materialize has_access owner rows for has_instance pairs lacking one.
async fn backfill_owners(services: &Services, user_id: &str) -> Result<usize> {
    let ownership = services.relations().list_by_type("has_instance").await?;
    let mut created = 0usize;

    for rel in &ownership {
        // Any existing has_access row for this (from, to) pair?
        let existing = services
            .relations()
            .get_asset_relations(&rel.from_asset_id)
            .await?;
        let has_row = existing.iter().any(|r| {
            r.relation_type == RelationType::HasAccess && r.to_asset_id == rel.to_asset_id
        });
        if has_row {
            continue;
        }
        let request = CreateRelationRequest {
            from_asset_id: rel.from_asset_id.clone(),
            to_asset_id: rel.to_asset_id.clone(),
            relation_type: RelationType::HasAccess,
            metadata: [(
                "role".to_string(),
                serde_json::Value::String(MEMBERSHIP_ROLE_OWNER.to_string()),
            )]
            .into_iter()
            .collect(),
        };
        services.relations().create(request, user_id).await?;
        created += 1;
    }

    Ok(created)
}

/// Stamp `global_role` on every asset of the given type when absent.
async fn backfill_global_role(
    services: &Services,
    user_id: &str,
    asset_type: &str,
    role: &str,
) -> Result<usize> {
    let mut tagged = 0usize;
    let mut cursor: Option<String> = None;

    loop {
        // Tag-filtered scan via search — the list path filters category
        // 'asset_type' while the live registry convention (tocpi births, org
        // tools) is 'type'; search binds the category explicitly.
        let (assets, next) = services
            .assets()
            .search(
                None,
                vec![("type".to_string(), asset_type.to_string())],
                BACKFILL_PAGE,
                cursor.as_deref(),
            )
            .await?;

        for asset in &assets {
            if asset.tags.iter().any(|t| t.category == "global_role") {
                continue;
            }
            services
                .assets()
                .add_tag(
                    &asset.id,
                    AddTagRequest {
                        category: "global_role".to_string(),
                        value: role.to_string(),
                    },
                    user_id,
                )
                .await?;
            tagged += 1;
        }

        match next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    Ok(tagged)
}
