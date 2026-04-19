// PDT Role-Based Access Control Policies
//
// These policies control who can perform which actions on assets.
// They work alongside the entity schema defined in schema.cedarschema.

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
