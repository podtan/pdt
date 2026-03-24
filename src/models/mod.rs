//! Data models for PDT

mod asset;
mod audit;
mod collection;
mod relation;
mod tag;

pub use asset::{generate_snippet, Asset, CreateAssetRequest, SearchResult, TagSummary, UpdateAssetRequest};
pub use audit::{AuditAction, AuditEntry};
pub use collection::{
    AddAssetRequest, Collection, CreateCollectionRequest, UpdateCollectionRequest,
};
pub use relation::{CreateRelationRequest, Relation, RelationType};
pub use tag::{validate_category, validate_value, AddTagRequest, Tag};

use serde::{Deserialize, Serialize};

/// Custom datetime serialization that writes:
/// - **JSON** (human-readable): RFC3339 strings for API responses
/// - **BSON** (non-human-readable): native BSON datetimes for MongoDB storage
///
/// Deserialization handles both native BSON datetimes, RFC3339 strings (legacy data),
/// and extended JSON `{"$date": ...}` format for full backward compatibility.
pub mod datetime_format {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if serializer.is_human_readable() {
            // JSON API responses: RFC3339 string
            serializer.serialize_str(&date.to_rfc3339())
        } else {
            // BSON/MongoDB storage: native BSON datetime
            let bson_dt = bson::DateTime::from_chrono(*date);
            bson_dt.serialize(serializer)
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        // Support native BSON datetime, RFC3339 strings (legacy), and extended JSON
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum DateTimeFormat {
            BsonNative(bson::DateTime),
            Rfc3339(String),
            BsonExtJson(BsonDateTimeExtJson),
        }

        #[derive(Deserialize)]
        struct BsonDateTimeExtJson {
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
            DateTimeFormat::BsonNative(bson_dt) => Ok(bson_dt.to_chrono()),
            DateTimeFormat::Rfc3339(s) => DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| D::Error::custom(format!("Invalid datetime format: {}", e))),
            DateTimeFormat::BsonExtJson(ext) => {
                let millis = match ext.date {
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
