//! Audit service

use crate::db::Database;
use crate::error::Result;
use crate::models::AuditEntry;
use crate::repository::AuditRepository;

/// Service for audit business logic
pub struct AuditService;

impl AuditService {
    /// List audit entries with filters
    pub async fn list(
        db: &Database,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)> {
        AuditRepository::list(db, entity_type, entity_id, user_id, limit, cursor).await
    }

    /// Get history for a specific asset
    pub async fn get_asset_history(
        db: &Database,
        asset_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditEntry>> {
        AuditRepository::get_entity_history(db, "asset", asset_id, limit).await
    }
}
