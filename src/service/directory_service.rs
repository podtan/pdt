//! Directory service — uuid → {name, kind, email/role} name-map (4474a038).
//!
//! Directory entries are PDT data: assets tagged `type=directory_entry` with
//! a unique `principal={uuid}` tag, metadata {principal_uuid, kind, email,
//! role}, title = display name. Populated from nghr employees, thq directory
//! rows, and tocpi user records via the upsert API or the idempotent
//! backfill. Resolution is decoupled from Kanidm (WS-A ruling).
//!
//! AC (4474a038): every known principal resolves; unknown id → LOUD
//! unresolved marker (never fake names); backfill idempotent.

use std::collections::HashMap;

use crate::error::{ApiError, Result};
use crate::models::directory::{
    DirectoryEntry, UpsertDirectoryRequest, DIRECTORY_ENTRY_TYPE, PRINCIPAL_TAG,
};
use crate::models::{AddTagRequest, CreateAssetRequest};
use crate::service::Services;

/// Page size for backfill scans.
const BACKFILL_PAGE: i64 = 200;

/// Directory service — uuid → {name, kind, email/role} name-map (4474a038).
pub struct DirectoryService;

impl DirectoryService {
    /// Resolve a principal uuid to its directory entry.
    ///
    /// Unknown id → LOUD unresolved marker (404 with the uuid echoed) — never a
    /// fake name (4474a038 AC).
    pub async fn resolve(services: &Services, uuid: &str) -> Result<DirectoryEntry> {
        if uuid.is_empty() {
            return Err(ApiError::BadRequest("uuid must be non-empty".to_string()));
        }
        match find_entry(services, uuid).await? {
            Some(entry) => Ok(entry),
            None => Err(ApiError::NotFound(format!(
                "unresolved_principal: no directory entry for uuid {}",
                uuid
            ))),
        }
    }

    /// Upsert a directory entry (import path for nghr employees, thq directory
    /// rows, tocpi user records). Idempotent by uuid: an existing entry for the
    /// uuid is updated in place; otherwise one is created.
    pub async fn upsert(
        services: &Services,
        uuid: &str,
        request: UpsertDirectoryRequest,
        user_id: &str,
    ) -> Result<DirectoryEntry> {
        if uuid.is_empty() {
            return Err(ApiError::BadRequest("uuid must be non-empty".to_string()));
        }

        let title = request
            .name
            .clone()
            .unwrap_or_else(|| format!("directory:{}", uuid));
        let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();
        metadata.insert(
            "principal_uuid".to_string(),
            serde_json::Value::String(uuid.to_string()),
        );
        if let Some(kind) = &request.kind {
            metadata.insert("kind".to_string(), serde_json::Value::String(kind.clone()));
        }
        if let Some(email) = &request.email {
            metadata.insert(
                "email".to_string(),
                serde_json::Value::String(email.clone()),
            );
        }
        if let Some(role) = &request.role {
            metadata.insert("role".to_string(), serde_json::Value::String(role.clone()));
        }

        match find_entry(services, uuid).await? {
            Some(_existing) => {
                // Update in place — resolve the backing asset id from the entry.
                let backing = find_backing_asset(services, uuid).await?.ok_or_else(|| {
                    ApiError::Internal("directory entry vanished mid-upsert".to_string())
                })?;
                services
                    .assets()
                    .update(
                        &backing.id,
                        crate::models::UpdateAssetRequest {
                            title: Some(title),
                            content: None,
                            metadata: Some(metadata),
                        },
                        user_id,
                    )
                    .await?;
            }
            None => {
                let request = CreateAssetRequest {
                    title,
                    content: None,
                    tags: vec![
                        AddTagRequest {
                            category: "type".to_string(),
                            value: DIRECTORY_ENTRY_TYPE.to_string(),
                        },
                        AddTagRequest {
                            category: PRINCIPAL_TAG.to_string(),
                            value: uuid.to_string(),
                        },
                    ],
                    metadata,
                    auth_context: None,
                };
                services.assets().create(request, user_id).await?;
            }
        }

        Self::resolve(services, uuid).await
    }

    /// List directory entries (paginated).
    pub async fn list(
        services: &Services,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<DirectoryEntry>, Option<String>)> {
        let (assets, next) = services
            .assets()
            .search(
                None,
                vec![("type".to_string(), DIRECTORY_ENTRY_TYPE.to_string())],
                limit,
                cursor,
            )
            .await?;
        let entries = assets
            .into_iter()
            .filter_map(|a| asset_to_entry(&a))
            .collect();
        Ok((entries, next))
    }

    /// Backfill directory entries from registry user/agent assets (4474a038) —
    /// IDEMPOTENT. Derives uuid → {name, kind, email} from what PDT already
    /// holds: sub-tagged user assets (tocpi user records; agent users with raw
    /// UUID emails get display names from their username tag) and agent assets
    /// (agent directory rows). Re-runs clean: existing entries are refreshed,
    /// absent ones created.
    pub async fn backfill(services: &Services, user_id: &str) -> Result<serde_json::Value> {
        let mut entries_upserted = 0usize;

        for asset_type in ["user", "agent"] {
            let mut cursor: Option<String> = None;
            loop {
                let (assets, next) = services
                    .assets()
                    .search(
                        None,
                        vec![("type".to_string(), asset_type.to_string())],
                        BACKFILL_PAGE,
                        cursor.as_deref(),
                    )
                    .await?;

                for asset in &assets {
                    let uuid = asset
                        .tags
                        .iter()
                        .find(|t| t.category == "sub")
                        .map(|t| t.value.clone())
                        .unwrap_or_else(|| asset.id.clone());
                    let kind = if asset_type == "agent" {
                        "agent"
                    } else {
                        "human"
                    };
                    let email = asset
                        .tags
                        .iter()
                        .find(|t| t.category == "email")
                        .map(|t| t.value.clone());
                    // Display name: username tag first; the title only when it is
                    // not the raw uuid/uuid-email form (agent users show raw UUID
                    // emails today — never surface one as a "name").
                    let name = asset
                        .tags
                        .iter()
                        .find(|t| t.category == "username")
                        .map(|t| t.value.clone())
                        .or_else(|| {
                            let title = asset.title.as_str();
                            let raw_uuid_title = title == uuid || email.as_deref() == Some(title);
                            if raw_uuid_title {
                                None
                            } else {
                                Some(title.to_string())
                            }
                        });

                    // Idempotent: an existing entry for this uuid is left
                    // as-is (the upsert API is the explicit import/refresh
                    // path) — re-runs clean, count reports true changes only.
                    if Self::resolve(services, &uuid).await.is_ok() {
                        continue;
                    }
                    let request = UpsertDirectoryRequest {
                        name,
                        kind: Some(kind.to_string()),
                        email,
                        role: None,
                    };
                    Self::upsert(services, &uuid, request, user_id).await?;
                    entries_upserted += 1;
                }

                match next {
                    Some(c) => cursor = Some(c),
                    None => break,
                }
            }
        }

        let summary: serde_json::Value =
            serde_json::json!({ "directory_entries_upserted": entries_upserted });
        Ok(summary)
    }
}

/// Find a directory entry asset by its principal tag and map it.
async fn find_entry(services: &Services, uuid: &str) -> Result<Option<DirectoryEntry>> {
    let backing = find_backing_asset(services, uuid).await?;
    Ok(backing.as_ref().and_then(asset_to_entry))
}

/// Find the backing asset for a principal uuid (principal tag lookup).
async fn find_backing_asset(
    services: &Services,
    uuid: &str,
) -> Result<Option<crate::models::Asset>> {
    let (assets, _) = services
        .assets()
        .search(
            None,
            vec![(PRINCIPAL_TAG.to_string(), uuid.to_string())],
            1,
            None,
        )
        .await?;
    Ok(assets.into_iter().next())
}

/// Map a backing asset to its directory entry (None when the asset is not a
/// directory entry).
fn asset_to_entry(asset: &crate::models::Asset) -> Option<DirectoryEntry> {
    let is_entry = asset
        .tags
        .iter()
        .any(|t| t.category == "type" && t.value == DIRECTORY_ENTRY_TYPE);
    if !is_entry {
        return None;
    }
    let meta_str = |key: &str| -> Option<String> {
        asset
            .metadata
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };
    Some(DirectoryEntry {
        uuid: meta_str("principal_uuid").unwrap_or_else(|| asset.id.clone()),
        name: Some(asset.title.clone()).filter(|t| !t.is_empty() && !t.starts_with("directory:")),
        kind: meta_str("kind"),
        email: meta_str("email"),
        role: meta_str("role"),
    })
}
