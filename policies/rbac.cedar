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

permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "team"
};

// Only users with admin role can delete assets
permit(
    principal,
    action == Action::"Delete",
    resource
) when {
    principal has role && principal.role == "admin"
};

// Users can edit assets they created (ownership policy)
permit(
    principal,
    action == Action::"Edit",
    resource
) when {
    resource has created_by && principal in resource.created_by
};

// Team visibility: users in owner_groups can view team-scoped assets
permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "team"
    && resource has owner_groups
    && principal has groups
    && resource.owner_groups.containsAny(principal.groups)
};

// Team edit: users in owner_groups can edit team-scoped assets
permit(
    principal,
    action == Action::"Edit",
    resource
) when {
    resource has visibility && resource.visibility == "team"
    && resource has owner_groups
    && principal has groups
    && resource.owner_groups.containsAny(principal.groups)
};

// Private visibility: only the creator can view
permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has created_by && principal in resource.created_by
};
