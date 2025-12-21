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
pub use tag::{validate_category, validate_value, AddTagRequest, Tag};

use serde::{Deserialize, Serialize};

/// Custom datetime serialization for API responses (ISO 8601 format)
/// MongoDB stores dates in BSON format, but we want ISO 8601 strings in JSON API responses
pub mod datetime_format {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&date.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Support both ISO 8601 strings and BSON datetime format
        use serde::de::Error;

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum DateTimeFormat {
            Rfc3339(String),
            Bson(BsonDateTime),
        }

        #[derive(Deserialize)]
        struct BsonDateTime {
            #[serde(rename = "$date")]
            date: BsonDateInner,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum BsonDateInner {
            NumberLong {
                #[serde(rename = "$numberLong")]
                number_long: String,
            },
            Millis(i64),
        }

        match DateTimeFormat::deserialize(deserializer)? {
            DateTimeFormat::Rfc3339(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| D::Error::custom(format!("Invalid datetime format: {}", e))),
            DateTimeFormat::Bson(bson) => {
                let millis = match bson.date {
                    BsonDateInner::NumberLong { number_long } => number_long
                        .parse::<i64>()
                        .map_err(|e| D::Error::custom(format!("Invalid numberLong: {}", e)))?,
                    BsonDateInner::Millis(m) => m,
                };
                DateTime::from_timestamp_millis(millis)
                    .ok_or_else(|| D::Error::custom("Invalid timestamp"))
            }
        }
    }
}

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
