//! Relation service

use serde_json::json;

use crate::error::{ApiError, Result};
use crate::models::{AuditAction, CreateRelationRequest, Relation};
use crate::service::Services;

/// Service for relation business logic
pub struct RelationService;

impl RelationService {
    /// Create a new relation with validation
    pub async fn create(
        services: &Services,
        request: CreateRelationRequest,
        user_id: &str,
    ) -> Result<Relation> {
        // Validate source asset exists
        if !services.assets().exists(&request.from_asset_id).await? {
            return Err(ApiError::NotFound(format!(
                "Source asset not found: {}",
                request.from_asset_id
            )));
        }

        // Validate target asset exists
        if !services.assets().exists(&request.to_asset_id).await? {
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
        if services
            .relations()
            .would_create_cycle(&request.from_asset_id, &request.to_asset_id)
            .await?
        {
            return Err(ApiError::Validation(
                "Creating this relation would create a cycle".to_string(),
            ));
        }

        // Create relation
        let relation = services.relations().create(request.clone(), user_id).await?;

        // Create audit entry
        services.audit().create(
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
    pub async fn get(services: &Services, id: &str) -> Result<Relation> {
        services.relations().get_by_id(id).await
    }

    /// Delete a relation
    pub async fn delete(services: &Services, id: &str, user_id: &str) -> Result<()> {
        // Get relation for audit
        let relation = services.relations().get_by_id(id).await?;

        // Delete relation
        services.relations().delete(id).await?;

        // Create audit entry
        services.audit().create(
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
    pub async fn get_asset_relations(services: &Services, asset_id: &str) -> Result<Vec<Relation>> {
        // Verify asset exists
        services.assets().get_by_id(asset_id).await?;

        services.relations().get_asset_relations(asset_id).await
    }

    /// Traverse the relationship graph
    pub async fn traverse_graph(
        services: &Services,
        asset_id: &str,
        max_depth: u32,
    ) -> Result<Vec<GraphNode>> {
        // Verify asset exists
        services.assets().get_by_id(asset_id).await?;

        let traversal = services.relations().traverse_graph(asset_id, max_depth).await?;

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
