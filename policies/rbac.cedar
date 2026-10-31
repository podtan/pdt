// PDT Role-Based Access Control Policies
//
// These policies control who can perform which actions on assets.
// They work alongside the entity schema defined in schema.cedarschema.

// Service-to-service: NGHR service account (nghr_svc) has full write access.
// nghr_svc@idm.tanbal.ir UUID: 0b40e1ae-8c3b-473c-bf90-1a30027c4e41
// Members of pdt_admins group (which nghr_svc is a member of) get admin role
// via the pdt-api claim map, so the admin role policies below already cover it.
// This explicit policy ensures forward-compatibility for any service-level checks.
permit(
    principal,
    action in [Action::"View", Action::"Create", Action::"Edit", Action::"Delete", Action::"Tag", Action::"Relate"],
    resource
) when {
    principal has role && principal.role == "admin"
};

// ─── Instance type protection ───
// Only admins can create or modify assets tagged as instance-type.
// This prevents non-admin users from injecting new instance types via PDT API directly.
forbid(
    principal,
    action in [Action::"Create", Action::"Edit", Action::"Delete", Action::"Tag"],
    resource
) when {
    resource has asset_type && resource.asset_type == "instance-type"
    && !(principal has role && principal.role == "admin")
};

// All authenticated users can view assets (unless restricted by visibility/confidentiality)
permit(
    principal,
    action == Action::"View",
    resource
) when {
    !(resource has visibility)
};

permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "public"
};

permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "org"
};

// Only users with admin role can delete assets
permit(
    principal,
    action == Action::"Delete",
    resource
) when {
    principal has role && principal.role == "admin"
};

// Owners can delete assets they created (ownership policy — mirrors the
// created_by Edit policy below). Delete-gap fix (Farzan ruling d130b9b5,
// 2026-10-17): a user who can create a workspace but can never delete it is
// an orphan factory; this keeps caller-credential attribution and does NOT
// promote group-based members (created_by is creator-only).
permit(
    principal,
    action == Action::"Delete",
    resource
) when {
    resource has created_by && principal in resource.created_by
};

// Users can edit assets they created (ownership policy)
permit(
    principal,
    action == Action::"Edit",
    resource
) when {
    resource has created_by && principal in resource.created_by
};

// Team visibility: users in owner_groups can view team-scoped assets.
// Sub/SPN-keyed arm (CPO ruling eda88e15, 2026-10-23): a byte-exact
// principal-sub (or SPN) match in owner_groups grants access independent
// of the groups claim — exchanged/machine tokens carry no groups claim
// (6c8efe55 class) and must not lose creator-declared access. owner_groups
// is creator-declared/enforced-at-edit, so this grants nothing the creator
// did not already list — no widening.
permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "team"
    && resource has owner_groups
    && (
        (principal has groups && resource.owner_groups.containsAny(principal.groups))
        || (principal has sub && resource.owner_groups.contains(principal.sub))
        || (principal has spn && resource.owner_groups.contains(principal.spn))
    )
};

// Team edit: users in owner_groups can edit team-scoped assets.
// Sub/SPN-keyed arm (CPO ruling eda88e15, 2026-10-23): Edit carries the
// identical hole to View — same sub/SPN arm, one policy change covering
// both arms.
permit(
    principal,
    action == Action::"Edit",
    resource
) when {
    resource has visibility && resource.visibility == "team"
    && resource has owner_groups
    && (
        (principal has groups && resource.owner_groups.containsAny(principal.groups))
        || (principal has sub && resource.owner_groups.contains(principal.sub))
        || (principal has spn && resource.owner_groups.contains(principal.spn))
    )
};

// Private visibility: only the creator can view
permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has created_by && principal in resource.created_by
};

// All authenticated users can create, tag, and relate assets
permit(
    principal,
    action == Action::"Create",
    resource
);

permit(
    principal,
    action == Action::"Tag",
    resource
);

permit(
    principal,
    action == Action::"Relate",
    resource
);

// ─── Workspace membership + agent management (WS-B 99035dee) ───
// Membership data (has_access relations) and agent links (has_agent
// relations) are the role ladder — the generic Relate permit above must
// NEVER cover them (any authenticated user could self-escalate via a
// direct has_access write). These writes enforce the dedicated actions
// with the caller's RESOLVED seat role in the context.
//
// Platform admins retain everything (mirrors the admin permit above).
permit(
    principal,
    action in [Action::"ManageMembers", Action::"ManageWorkspaceAgents"],
    resource
) when {
    principal has role && principal.role == "admin"
};

// Owner retains everything on their own workspace (the caller_role is
// resolved FOR the target workspace — a role on workspace A grants
// nothing on workspace B; the cross-workspace negative is the default
// deny with no existence leak: a uniform 403 either way).
permit(
    principal,
    action == Action::"ManageMembers",
    resource
) when {
    context has caller_role && context.caller_role == "owner"
};

permit(
    principal,
    action == Action::"ManageWorkspaceAgents",
    resource
) when {
    context has caller_role && context.caller_role == "owner"
};

// ws_admin manages PLAIN members only — never the role ladder
// (Farzan ruling d130b9b5: WsAdmin manages members, never the role
// ladder; self-escalation denied): may add a member seat onto a
// non-member and remove a plain member; may never touch ws_admin/owner
// seats, grant a ws_admin seat, or change an existing seat's role.
permit(
    principal,
    action == Action::"ManageMembers",
    resource
) when {
    context has caller_role && context.caller_role == "ws_admin"
    && context has target_seat_role
    && (context.target_seat_role == "none" || context.target_seat_role == "member")
    && (
        !(context has requested_role)
        || (context has requested_role && context.requested_role == "member")
    )
};

// ws_admin manages the workspace's agents (use-plane administration —
// d130b9b5), within their own workspace only.
permit(
    principal,
    action == Action::"ManageWorkspaceAgents",
    resource
) when {
    context has caller_role && context.caller_role == "ws_admin"
};
