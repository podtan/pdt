//! OpenAPI spec definition using utoipa.
//!
//! The `ApiDoc` struct aggregates all paths (handler annotations) and
//! schemas (ToSchema derives) so that the spec can be served at runtime
//! via `/api-docs/openapi.json` and fetched by `scripts/generate-openapi.sh`.

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Platform Data Toolkit (PDT) API",
        description = "PDT is a centralized API server that serves as an enterprise knowledge silo for company-specific concepts, documents, and their relationships.\n\n## Features\n\n- **Knowledge Asset Management**: Store and manage knowledge assets with rich metadata\n- **Tag-Based Classification**: Multi-dimensional tagging including asset type, business domain, language, etc.\n- **Graph-Based Relationships**: Define and traverse relationships between assets\n- **Concept Collections**: Organize assets into named collections\n- **Search & Discovery**: Full-text search and tag-based filtering\n- **Audit Logging**: Track all changes with user attribution\n- **Authorization Context**: Cedar-based visibility, owner groups, and confidentiality management",
        version = "1.0.0",
        contact(name = "PDT Team"),
        license(name = "MIT OR Apache-2.0"),
    ),
    servers(
        (url = "http://localhost:8080", description = "Local development server"),
    ),
    paths(
        crate::handlers::health::health_check,
        // Asset routes
        crate::handlers::assets::create_asset,
        crate::handlers::assets::list_assets,
        crate::handlers::assets::get_asset,
        crate::handlers::assets::update_asset,
        crate::handlers::assets::merge_asset_metadata,
        crate::handlers::assets::delete_asset,
        crate::handlers::assets::add_tag,
        crate::handlers::assets::remove_tag,
        crate::handlers::assets::update_auth_context,
        crate::handlers::assets::adopt_asset,
        // Relation routes
        crate::handlers::relations::create_relation,
        crate::handlers::relations::get_relation,
        crate::handlers::relations::delete_relation,
        crate::handlers::relations::get_asset_relations,
        crate::handlers::relations::traverse_graph,
        // Collection routes
        crate::handlers::collections::create_collection,
        crate::handlers::collections::list_collections,
        crate::handlers::collections::get_collection,
        crate::handlers::collections::update_collection,
        crate::handlers::collections::delete_collection,
        crate::handlers::collections::add_asset,
        crate::handlers::collections::remove_asset,
        // Search routes
        crate::handlers::search::search,
        // Role model (1552c098) + directory name-map (4474a038) + maintenance
        crate::handlers::roles::get_roles,
        crate::handlers::directory::get_directory_entry,
        crate::handlers::directory::upsert_directory_entry,
        crate::handlers::directory::list_directory,
        crate::handlers::maintenance::backfill,
        crate::handlers::maintenance::prune,
        crate::handlers::maintenance::audit_prune,
        crate::handlers::maintenance::reconcile,
        // Audit routes
        crate::handlers::audit::list_audit_entries,
        crate::handlers::audit::get_asset_history,
        // Cedar policy store
        crate::handlers::cedar::get_cedar_policies,
        // Instance (multi-tenant) routes
        crate::handlers::instances::provision_instance,
        crate::handlers::instances::delete_instance,
        crate::handlers::instances::list_instances,
        crate::handlers::instances::list_instance_tree,
        crate::handlers::instances::list_instance_sizes,
        // Group routes (PDT-native per-instance groups, a9f91cfb)
        crate::handlers::group::create_group,
        crate::handlers::group::list_groups,
        crate::handlers::group::get_group,
        crate::handlers::group::delete_group,
        crate::handlers::group::add_group_member,
        crate::handlers::group::remove_group_member,
        crate::handlers::group::list_group_members,
        crate::handlers::group::resolve_member_groups,
    ),
    components(schemas(
        crate::handlers::health::HealthResponse,
        crate::models::Asset,
        crate::models::CreateAssetRequest,
        crate::models::AdoptAssetRequest,
        crate::models::UpdateAssetRequest,
        crate::models::MergeMetadataRequest,
        crate::models::UpdateAuthContextRequest,
        crate::models::AuthContext,
        crate::models::Tag,
        crate::models::AddTagRequest,
        crate::models::SearchResult,
        crate::models::TagSummary,
        crate::models::Relation,
        crate::models::RelationType,
        crate::models::CreateRelationRequest,
        crate::models::Collection,
        crate::models::CreateCollectionRequest,
        crate::models::UpdateCollectionRequest,
        crate::models::AddAssetRequest,
        crate::models::AuditEntry,
        crate::models::AuditAction,
        crate::handlers::maintenance::AuditPruneRequest,
        crate::handlers::maintenance::ReconcileRequest,
        crate::handlers::instances::InstanceStoreSize,
        crate::service::relation_service::GraphNode,
        crate::handlers::cedar::CedarPolicyResponse,
        crate::models::roles::RoleAnswer,
        crate::models::roles::MembershipRoleRow,
        crate::models::directory::DirectoryEntry,
        crate::models::directory::UpsertDirectoryRequest,
        crate::models::group::Group,
        crate::models::group::GroupMember,
        crate::models::group::CreateGroupRequest,
        crate::models::group::AddGroupMemberRequest,
        crate::models::group::MemberGroupsResponse,
        crate::models::group::GroupMembersResponse,
    )),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "assets", description = "Knowledge asset management"),
        (name = "relations", description = "Asset relationship management"),
        (name = "collections", description = "Asset collection management"),
        (name = "search", description = "Full-text search and discovery"),
        (name = "roles", description = "Sub-keyed role resolution (membership + global)"),
        (name = "directory", description = "Principal directory name-map (uuid → name/kind/email/role)"),
        (name = "groups", description = "PDT-native per-instance groups (authorization decoupled from Kanidm)"),
        (name = "maintenance", description = "Admin-gated idempotent backfills + retention prune (dry-run default, execution wire-gated)"),
        (name = "audit", description = "Audit logging and history"),
        (name = "cedar", description = "Cedar policy store"),
    ),
)]
pub struct ApiDoc;
