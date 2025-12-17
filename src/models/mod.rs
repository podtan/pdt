//! Data models for PDT

mod asset;
mod audit;
mod collection;
mod relation;
mod tag;

pub use asset::{Asset, CreateAssetRequest, UpdateAssetRequest};
pub use audit::{AuditAction, AuditEntry};
pub use collection::{
    AddAssetRequest, Collection, CreateCollectionRequest, UpdateCollectionRequest,
};
pub use relation::{CreateRelationRequest, Relation, RelationType};
pub use tag::{AddTagRequest, Tag, TagCategory};

use serde::{Deserialize, Serialize};

/// Pagination parameters
#[derive(Debug, Clone, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

/// Paginated response wrapper
#[derive(Debug, Clone, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub next_cursor: Option<String>,
    pub total: Option<u64>,
}
