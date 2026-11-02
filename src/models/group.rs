//! PDT-native per-instance groups (workstream 69b4bb4c, task a9f91cfb)
//!
//! Groups live per-instance by construction — the store IS the instance
//! (PDT speaks instance, not workspace; owner's correction 2026-11-02).
//! A distinguished `instance-admins` group is seeded in every store
//! (migration 002): members see all assets within the instance (audited
//! admin bypass) and manage the instance's groups.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::datetime_format;

/// The distinguished per-instance admin group name (canonical band
/// reference — bands store group NAMES, membership resolves at read time).
pub const INSTANCE_ADMINS_GROUP: &str = "instance-admins";

/// The distinguished instance-admins group's stable id (seeded by
/// migration 002 in every instance store).
pub const INSTANCE_ADMINS_GROUP_ID: &str = "00000000-0000-0000-0000-0000000000a1";

/// A PDT-native group.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Group {
    #[serde(rename = "_id")]
    pub id: String,
    /// Canonical group name — bands reference THIS (membership resolves,
    /// not string forms).
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(with = "datetime_format")]
    pub created_at: DateTime<Utc>,
    pub created_by: String,
}

/// A group membership row.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct GroupMember {
    pub group_id: String,
    /// The principal's stable id (token sub / agent id).
    pub member_id: String,
    pub added_by: String,
    #[serde(with = "datetime_format")]
    pub added_at: DateTime<Utc>,
}

/// Request body: create a group.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CreateGroupRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Request body: add a member to a group.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct AddGroupMemberRequest {
    /// The principal's stable id (token sub / agent id).
    pub member_id: String,
}

/// Response: the groups a principal belongs to (read-time resolution
/// surface — names, not IDs).
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct MemberGroupsResponse {
    pub member_id: String,
    pub groups: Vec<String>,
}

/// Response: a group's members.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GroupMembersResponse {
    pub group_id: String,
    pub members: Vec<GroupMember>,
}
