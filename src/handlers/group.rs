//! Group management handlers (workstream 69b4bb4c, task a9f91cfb —
//! PDT-native per-instance groups).
//!
//! Management surface: REST + MCP tools (the pdt MCP bridge proxies this
//! REST surface). Instant, audited in audit_entries; no GitOps, no Kanidm
//! admin token.
//!
//! Authorization: platform admins (role=admin) and instance admins
//! (members of the distinguished instance-admins group) manage groups.
//! Reads (list/get/members/resolve) are open to any authenticated caller
//! — group membership is authorization DATA, not a secret.

use axum::{extract::Path, Json};

use crate::auth::AuthenticatedUser;
use crate::cedar::enforcement::TenantState;
use crate::error::{ApiError, Result};
use crate::models::{
    AddGroupMemberRequest, CreateGroupRequest, Group, GroupMembersResponse, MemberGroupsResponse,
};
use crate::service::group_service::GroupService;

/// Is this caller allowed to MANAGE groups in this instance?
/// Platform admin (role=admin) OR instance admin (instance-admins member).
async fn can_manage_groups(tx: &TenantState, user: &AuthenticatedUser) -> Result<bool> {
    let claims = user.to_cedar_claims();
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
    if is_platform_admin {
        return Ok(true);
    }
    // Instance admin: member of the distinguished group (PDT-native,
    // fail-closed on resolve failure — the bypass is a widening).
    let is_instance_admin = tx
        .services
        .groups()
        .is_instance_admin(&claims.sub)
        .await
        .unwrap_or(false);
    Ok(is_instance_admin)
}

async fn require_group_manager(tx: &TenantState, user: &AuthenticatedUser) -> Result<()> {
    if !can_manage_groups(tx, user).await? {
        return Err(ApiError::Forbidden(
            "Access denied: group management requires the platform admin role or instance-admins membership".to_string(),
        ));
    }
    Ok(())
}

/// Create a group.
#[utoipa::path(
    post,
    path = "/api/groups",
    request_body = CreateGroupRequest,
    responses(
        (status = 200, description = "Group created", body = Group),
        (status = 400, description = "Empty name"),
        (status = 403, description = "Requires platform admin or instance-admins membership"),
        (status = 409, description = "Group name already exists in this instance"),
    ),
    tag = "groups",
)]
pub async fn create_group(
    tx: TenantState,
    user: AuthenticatedUser,
    Json(request): Json<CreateGroupRequest>,
) -> Result<Json<Group>> {
    require_group_manager(&tx, &user).await?;
    let group = GroupService::create(
        tx.services(),
        &request.name,
        &request.description,
        &user.user_id,
    )
    .await?;
    Ok(Json(group))
}

/// List all groups in this instance.
#[utoipa::path(
    get,
    path = "/api/groups",
    responses(
        (status = 200, description = "Groups in this instance", body = [Group]),
    ),
    tag = "groups",
)]
pub async fn list_groups(tx: TenantState) -> Result<Json<Vec<Group>>> {
    let groups = GroupService::list(tx.services()).await?;
    Ok(Json(groups))
}

/// Get a group by id.
#[utoipa::path(
    get,
    path = "/api/groups/{id}",
    params(
        ("id" = String, Path, description = "Group ID"),
    ),
    responses(
        (status = 200, description = "Group found", body = Group),
        (status = 404, description = "Group not found"),
    ),
    tag = "groups",
)]
pub async fn get_group(tx: TenantState, Path(id): Path<String>) -> Result<Json<Group>> {
    let group = GroupService::get(tx.services(), &id).await?;
    Ok(Json(group))
}

/// Delete a group (memberships cascade). The distinguished instance-admins
/// group cannot be deleted.
#[utoipa::path(
    delete,
    path = "/api/groups/{id}",
    params(
        ("id" = String, Path, description = "Group ID"),
    ),
    responses(
        (status = 200, description = "Group deleted"),
        (status = 400, description = "The distinguished instance-admins group cannot be deleted"),
        (status = 403, description = "Requires platform admin or instance-admins membership"),
        (status = 404, description = "Group not found"),
    ),
    tag = "groups",
)]
pub async fn delete_group(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    require_group_manager(&tx, &user).await?;
    GroupService::delete(tx.services(), &id, &user.user_id).await?;
    Ok(Json(serde_json::json!({ "deleted": id })))
}

/// Add a member to a group (audited; idempotent on re-add).
#[utoipa::path(
    post,
    path = "/api/groups/{id}/members",
    params(
        ("id" = String, Path, description = "Group ID"),
    ),
    request_body = AddGroupMemberRequest,
    responses(
        (status = 200, description = "Member added", body = crate::models::GroupMember),
        (status = 403, description = "Requires platform admin or instance-admins membership"),
        (status = 404, description = "Group not found"),
    ),
    tag = "groups",
)]
pub async fn add_group_member(
    tx: TenantState,
    user: AuthenticatedUser,
    Path(id): Path<String>,
    Json(request): Json<AddGroupMemberRequest>,
) -> Result<Json<crate::models::GroupMember>> {
    require_group_manager(&tx, &user).await?;
    let member =
        GroupService::add_member(tx.services(), &id, &request.member_id, &user.user_id).await?;
    Ok(Json(member))
}

/// Remove a member from a group (audited).
#[utoipa::path(
    delete,
    path = "/api/groups/{id}/members/{member_id}",
    params(
        ("id" = String, Path, description = "Group ID"),
        ("member_id" = String, Path, description = "The principal's stable id"),
    ),
    responses(
        (status = 200, description = "Member removed"),
        (status = 403, description = "Requires platform admin or instance-admins membership"),
    ),
    tag = "groups",
)]
pub async fn remove_group_member(
    tx: TenantState,
    user: AuthenticatedUser,
    Path((id, member_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    require_group_manager(&tx, &user).await?;
    GroupService::remove_member(tx.services(), &id, &member_id, &user.user_id).await?;
    Ok(Json(serde_json::json!({ "removed": member_id })))
}

/// List a group's members.
#[utoipa::path(
    get,
    path = "/api/groups/{id}/members",
    params(
        ("id" = String, Path, description = "Group ID"),
    ),
    responses(
        (status = 200, description = "Group members", body = GroupMembersResponse),
        (status = 404, description = "Group not found"),
    ),
    tag = "groups",
)]
pub async fn list_group_members(
    tx: TenantState,
    Path(id): Path<String>,
) -> Result<Json<GroupMembersResponse>> {
    let members = GroupService::list_members(tx.services(), &id).await?;
    Ok(Json(GroupMembersResponse {
        group_id: id,
        members,
    }))
}

/// Resolve a principal's group NAMES at read time (the Phase 1b surface —
/// names, not IDs; the cedar evaluator merges these into effective_groups).
#[utoipa::path(
    get,
    path = "/api/groups/member/{member_id}",
    params(
        ("member_id" = String, Path, description = "The principal's stable id (token sub / agent id)"),
    ),
    responses(
        (status = 200, description = "The principal's group names", body = MemberGroupsResponse),
    ),
    tag = "groups",
)]
pub async fn resolve_member_groups(
    tx: TenantState,
    Path(member_id): Path<String>,
) -> Result<Json<MemberGroupsResponse>> {
    let groups = GroupService::memberships_for(tx.services(), &member_id).await?;
    Ok(Json(MemberGroupsResponse { member_id, groups }))
}
