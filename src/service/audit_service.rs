//! Audit service

use crate::error::Result;
use crate::models::AuditEntry;
use crate::service::Services;

/// Service for audit business logic
pub struct AuditService;

impl AuditService {
    /// List audit entries with filters
    pub async fn list(
        services: &Services,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)> {
        services.audit().list(entity_type, entity_id, user_id, limit, cursor).await
    }

    /// Get history for a specific asset
    pub async fn get_asset_history(
        services: &Services,
        asset_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditEntry>> {
        services.audit().get_entity_history("asset", asset_id, limit).await
    }
}
