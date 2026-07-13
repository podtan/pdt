//! Relation service

use serde_json::json;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{AuditAction, CreateRelationRequest, Relation};
use crate::repository::{AssetRepository, AuditRepository, RelationRepository};

/// Service for relation business logic
pub struct RelationService;

impl RelationService {
    /// Create a new relation with validation
    pub async fn create(
        db: &Database,
        request: CreateRelationRequest,
        user_id: &str,
    ) -> Result<Relation> {
        // Validate source asset exists
        if !AssetRepository::exists(db, &request.from_asset_id).await? {
            return Err(ApiError::NotFound(format!(
                "Source asset not found: {}",
                request.from_asset_id
            )));
        }

        // Validate target asset exists
        if !AssetRepository::exists(db, &request.to_asset_id).await? {
            return Err(ApiError::NotFound(format!(
                "Target asset not found: {}",
                request.to_asset_id
            )));
        }

        // Prevent self-referential relations
        if request.from_asset_id == request.to_asset_id {
            return Err(ApiError::Validation(
                "Cannot create relation from asset to itself".to_string(),
            ));
        }

        // Check for cycles (for directional relation types)
        if RelationRepository::would_create_cycle(db, &request.from_asset_id, &request.to_asset_id)
            .await?
        {
            return Err(ApiError::Validation(
                "Creating this relation would create a cycle".to_string(),
            ));
        }

        // Create relation
        let relation = RelationRepository::create(db, request.clone(), user_id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "relation",
            &relation.id,
            AuditAction::Create,
            json!({
                "from_asset_id": relation.from_asset_id,
                "to_asset_id": relation.to_asset_id,
                "relation_type": format!("{}", relation.relation_type),
            }),
            user_id,
        )
        .await?;

        Ok(relation)
    }

    /// Get relation by ID
    pub async fn get(db: &Database, id: &str) -> Result<Relation> {
        RelationRepository::get_by_id(db, id).await
    }

    /// Delete a relation
    pub async fn delete(db: &Database, id: &str, user_id: &str) -> Result<()> {
        // Get relation for audit
        let relation = RelationRepository::get_by_id(db, id).await?;

        // Delete relation
        RelationRepository::delete(db, id).await?;

        // Create audit entry
        AuditRepository::create(
            db,
            "relation",
            id,
            AuditAction::RemoveRelation,
            json!({
                "from_asset_id": relation.from_asset_id,
                "to_asset_id": relation.to_asset_id,
                "relation_type": format!("{}", relation.relation_type),
            }),
            user_id,
        )
        .await?;

        Ok(())
    }

    /// Get all relations for an asset
    pub async fn get_asset_relations(db: &Database, asset_id: &str) -> Result<Vec<Relation>> {
        // Verify asset exists
        AssetRepository::get_by_id(db, asset_id).await?;

        RelationRepository::get_asset_relations(db, asset_id).await
    }

    /// Traverse the relationship graph
    pub async fn traverse_graph(
        db: &Database,
        asset_id: &str,
        max_depth: u32,
    ) -> Result<Vec<GraphNode>> {
        // Verify asset exists
        AssetRepository::get_by_id(db, asset_id).await?;

        let traversal = RelationRepository::traverse_graph(db, asset_id, max_depth).await?;

        let nodes = traversal
            .into_iter()
            .map(|(id, depth, relations)| GraphNode {
                asset_id: id,
                depth,
                relations,
            })
            .collect();

        Ok(nodes)
    }
}

/// Node in the relationship graph
#[derive(Debug, Clone, serde::Serialize, utoipa::ToSchema)]
pub struct GraphNode {
    pub asset_id: String,
    pub depth: u32,
    pub relations: Vec<Relation>,
}
