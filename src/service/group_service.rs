//! Group service (workstream 69b4bb4c, task a9f91cfb — PDT-native
//! per-instance groups).
//!
//! Groups live per-instance by construction (the store IS the instance).
//! Every mutation is audited in audit_entries (instant, no GitOps, no
//! Kanidm admin token). The distinguished `instance-admins` group is
//! guaranteed to exist in every store by `ensure_instance_admins_group`
//! — a BACKEND-AGNOSTIC bootstrap (the sqlite migration seeds it too;
//! this covers every backend, mongo today and postgres later).

use crate::error::{ApiError, Result};
use crate::models::{Group, GroupMember, INSTANCE_ADMINS_GROUP, INSTANCE_ADMINS_GROUP_ID};
use crate::service::Services;

pub struct GroupService;

impl GroupService {
    /// Backend-agnostic bootstrap: guarantee the distinguished
    /// instance-admins group exists in this store. Idempotent; called at
    /// every store open (main.rs global + tenant.rs per-instance).
    pub async fn ensure_instance_admins_group(services: &Services) -> Result<()> {
        if services
            .groups()
            .get_group_by_name(INSTANCE_ADMINS_GROUP)
            .await?
            .is_some()
        {
            return Ok(());
        }
        services.groups().create_group(
            INSTANCE_ADMINS_GROUP,
            "Distinguished per-instance admin group: members see all assets within this instance (audited admin bypass) and manage this instance's groups.",
            "system",
        ).await?;
        Ok(())
    }

    /// Seed bootstrap principals (token subs) into the instance-admins
    /// group — the DESIGNED seed mechanism (CPO ruling on defect c0c606c6,
    /// 2026-11-02, option (b)): an empty admin group at boot is
    /// born-invisible governance (the grey-icons class), and prod was
    /// never seeded while the IT env was (the IT/prod parity gap that
    /// blocked the tocpi propagate-admins backfill).
    ///
    /// Idempotent: an already-member sub is SKIPPED (no duplicate row, no
    /// duplicate audit entry on re-boot). Audited: each landing rides the
    /// SAME `add_member` path as every other mutation (one code path).
    /// Per-sub failures are WARN-loud and NON-FATAL — the store still
    /// opens (zero downtime); residue stays visible via the journal and
    /// the tocpi admin-drift detector. The subs come from
    /// `PDT_SEED_INSTANCE_ADMINS` (comma-separated) at the call sites;
    /// this core takes them explicitly so tests need no env mutation.
    pub async fn seed_instance_admins(
        services: &Services,
        subs: &[String],
        actor: &str,
    ) -> Result<()> {
        if subs.is_empty() {
            return Ok(());
        }
        let group = services
            .groups()
            .get_group_by_name(INSTANCE_ADMINS_GROUP)
            .await?
            .ok_or_else(|| {
                ApiError::Internal(
                    "instance-admins group missing at seed time (bootstrap order broken)"
                        .to_string(),
                )
            })?;
        let existing: Vec<String> = services
            .groups()
            .list_members(&group.id)
            .await?
            .into_iter()
            .map(|m| m.member_id)
            .collect();
        for sub in subs {
            let sub = sub.trim();
            if sub.is_empty() {
                continue;
            }
            if existing.iter().any(|m| m == sub) {
                tracing::debug!("Seed: {sub} already in instance-admins — skipped");
                continue;
            }
            match services.groups().add_member(&group.id, sub, actor).await {
                Ok(_) => {
                    let _ = services
                        .audit()
                        .create(
                            "group",
                            &group.id,
                            crate::models::AuditAction::AddMember,
                            serde_json::json!({
                                "member_id": sub,
                                "seed": true,
                            }),
                            actor,
                        )
                        .await;
                    tracing::info!("Seed: {sub} added to instance-admins (bootstrap seed)");
                }
                Err(e) => {
                    // WARN-loud, non-fatal: the store still opens; the
                    // residue is visible in the journal and via the
                    // tocpi admin-drift detector.
                    tracing::warn!("Seed: FAILED to add {sub} to instance-admins (non-fatal): {e}");
                }
            }
        }
        Ok(())
    }

    /// Create a group (audited).
    pub async fn create(
        services: &Services,
        name: &str,
        description: &str,
        user_id: &str,
    ) -> Result<Group> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ApiError::BadRequest(
                "group name must not be empty".to_string(),
            ));
        }
        // The distinguished group's name is reserved — it has ONE home
        // (the seeded row with the stable id); a second group by that
        // name would fork the admin-bypass gate.
        if name == INSTANCE_ADMINS_GROUP {
            return Err(ApiError::Conflict(
                "'instance-admins' is the distinguished per-instance admin group — it is seeded, not created".to_string(),
            ));
        }
        let group = services
            .groups()
            .create_group(name, description, user_id)
            .await?;
        let _ = services
            .audit()
            .create(
                "group",
                &group.id,
                crate::models::AuditAction::Create,
                serde_json::json!({ "name": group.name }),
                user_id,
            )
            .await?;
        Ok(group)
    }

    /// Get a group by id.
    pub async fn get(services: &Services, id: &str) -> Result<Group> {
        services.groups().get_group(id).await
    }

    /// List all groups in this instance store.
    pub async fn list(services: &Services) -> Result<Vec<Group>> {
        services.groups().list_groups().await
    }

    /// Delete a group (audited). The distinguished group is protected at
    /// the repository layer; the audit records the deletion.
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        let group = services.groups().get_group(id).await?;
        services.groups().delete_group(id).await?;
        let _ = services
            .audit()
            .create(
                "group",
                id,
                crate::models::AuditAction::Delete,
                serde_json::json!({ "name": group.name }),
                user_id,
            )
            .await?;
        Ok(())
    }

    /// Add a member to a group (audited). Idempotent on re-add.
    pub async fn add_member(
        services: &Services,
        group_id: &str,
        member_id: &str,
        user_id: &str,
    ) -> Result<GroupMember> {
        let member_id = member_id.trim();
        if member_id.is_empty() {
            return Err(ApiError::BadRequest(
                "member_id must not be empty".to_string(),
            ));
        }
        let member = services
            .groups()
            .add_member(group_id, member_id, user_id)
            .await?;
        let _ = services
            .audit()
            .create(
                "group",
                group_id,
                crate::models::AuditAction::AddMember,
                serde_json::json!({ "member_id": member_id }),
                user_id,
            )
            .await?;
        Ok(member)
    }

    /// Remove a member from a group (audited).
    pub async fn remove_member(
        services: &Services,
        group_id: &str,
        member_id: &str,
        user_id: &str,
    ) -> Result<()> {
        services.groups().remove_member(group_id, member_id).await?;
        let _ = services
            .audit()
            .create(
                "group",
                group_id,
                crate::models::AuditAction::RemoveMember,
                serde_json::json!({ "member_id": member_id }),
                user_id,
            )
            .await?;
        Ok(())
    }

    /// List a group's members.
    pub async fn list_members(services: &Services, group_id: &str) -> Result<Vec<GroupMember>> {
        services.groups().list_members(group_id).await
    }

    /// Resolve a principal's group NAMES at read time (Phase 1b surface —
    /// names, not IDs).
    pub async fn memberships_for(services: &Services, member_id: &str) -> Result<Vec<String>> {
        services.groups().memberships_for(member_id).await
    }
}
