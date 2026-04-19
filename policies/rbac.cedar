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
    !resource has visibility
    || resource.visibility == "public"
    || resource.visibility == "org"
    || resource.visibility == "team"
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
    resource has created_by && resource.created_by == principal
};

// Admins can edit any asset
permit(
    principal,
    action == Action::"Edit",
    resource
) when {
    principal has role && principal.role == "admin"
};

// Only admins can tag or relate assets
permit(
    principal,
    action == Action::"Tag",
    resource
) when {
    principal has role && principal.role == "admin"
    || resource has created_by && resource.created_by == principal
};

permit(
    principal,
    action == Action::"Relate",
    resource
) when {
    principal has role && principal.role == "admin"
    || resource has created_by && resource.created_by == principal
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
    && resource.owner_groups intersects principal.groups
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
    && resource.owner_groups intersects principal.groups
};

// Private visibility: only the creator can view
permit(
    principal,
    action == Action::"View",
    resource
) when {
    resource has visibility && resource.visibility == "private"
    && resource has created_by && resource.created_by == principal
};
