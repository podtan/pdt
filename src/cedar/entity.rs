//! Cedar entity builders for PDT
//!
//! Converts PDT `Asset` models and JWT claims into Cedar entities
//! for authorization evaluation.

use cedar_policy::{Entity, EntityId, EntityUid, RestrictedExpression};
use std::collections::{HashMap, HashSet};

use crate::models::Asset;

use pep::cedar::build_principal_entity;
use pep::oidc::types::JwtClaims;

/// Build a Cedar `Entity` from a PDT `Asset`.
///
/// Reads:
/// - `created_by` directly from Asset
/// - Auth-only fields from `auth_context` (visibility, owner_groups, confidentiality)
/// - Tag-derived fields from `asset.tags` at eval time (status, asset_type) — no duplication
pub fn asset_to_cedar_entity(asset: &Asset) -> Entity {
    let type_name: cedar_policy::EntityTypeName = "Asset".parse().unwrap();
    let uid = EntityUid::from_type_name_and_id(
        type_name,
        EntityId::new(&asset.id),
    );

    let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();

    // From Asset directly (no duplication)
    // Schema type: Set<User> — stores creator as a set of User entity UIDs
    // so that `principal in resource.created_by` works for ownership checks.
    let creator_uid: EntityUid = format!("User::\"{}\"", asset.created_by)
        .parse()
        .expect("Invalid creator UID");
    attrs.insert(
        "created_by".to_string(),
        RestrictedExpression::new_set(vec![
            RestrictedExpression::new_entity_uid(creator_uid),
        ]),
    );

    // From AuthContext (auth-only fields)
    if let Some(ref ctx) = asset.auth_context {
        attrs.insert(
            "visibility".to_string(),
            RestrictedExpression::new_string(format!("{}", ctx.visibility)),
        );
        attrs.insert(
            "confidentiality".to_string(),
            RestrictedExpression::new_string(format!("{}", ctx.confidentiality)),
        );
        if !ctx.owner_groups.is_empty() {
            let set_exprs: Vec<RestrictedExpression> = ctx
                .owner_groups
                .iter()
                .map(|g| RestrictedExpression::new_string(g.clone()))
                .collect();
            attrs.insert(
                "owner_groups".to_string(),
                RestrictedExpression::new_set(set_exprs),
            );
        }
    }

    // From Tags directly at eval time (no duplication, no sync needed)
    if let Some(t) = asset.tags.iter().find(|t| t.category == "status") {
        attrs.insert(
            "status".to_string(),
            RestrictedExpression::new_string(t.value.clone()),
        );
    }
    if let Some(t) = asset.tags.iter().find(|t| t.category == "asset_type") {
        attrs.insert(
            "asset_type".to_string(),
            RestrictedExpression::new_string(t.value.clone()),
        );
    }
    // Note: priority NOT included — it's task management, not access control

    let parents: HashSet<EntityUid> = HashSet::new();
    Entity::new(uid, attrs, parents).expect("Failed to build Cedar entity from Asset")
}

/// Build a Cedar resource `EntityUid` from an asset (for use in Cedar Requests).
pub fn build_asset_resource_uid(asset: &Asset) -> Result<EntityUid, pep::cedar::CedarError> {
    pep::cedar::entity::ResourceInfo::new("Asset", &asset.id).to_cedar_uid()
}

/// Build a Cedar principal `Entity` from JWT claims, including PDT-specific attributes.
///
/// Extends PEP's `build_principal_entity` with `role` and `groups` from extra claims
/// (matching the PDT schema's `User` entity).
pub fn user_to_cedar_principal(claims: &JwtClaims) -> Entity {
    // Start with PEP's base entity builder (email, name, username, roles)
    let base_entity = match build_principal_entity(claims) {
        Ok(e) => e,
        Err(_) => return Entity::new(
            pep::cedar::build_principal_uid(claims).unwrap(),
            HashMap::new(),
            HashSet::new(),
        ).unwrap(),
    };

    let uid = base_entity.uid().clone();
    let mut attrs: HashMap<String, RestrictedExpression> = HashMap::new();

    // PEP's base entity already has email, name, username from JWT claims.
    // We can't iterate Cedar Entity attrs directly, so we rebuild from claims here.

    // Add PDT-specific: role (single string from extra claims)
    if let Some(role) = claims.extra.get("role") {
        if let Some(role_str) = role.as_str() {
            attrs.insert(
                "role".to_string(),
                RestrictedExpression::new_string(role_str.to_string()),
            );
        }
    }

    // Add PDT-specific: groups (set from extra claims)
    if let Some(serde_json::Value::Array(groups)) = claims.extra.get("groups") {
        let set_exprs: Vec<RestrictedExpression> = groups
            .iter()
            .filter_map(|g| {
                if let serde_json::Value::String(s) = g {
                    Some(RestrictedExpression::new_string(s.clone()))
                } else {
                    None
                }
            })
            .collect();
        if !set_exprs.is_empty() {
            attrs.insert("groups".to_string(), RestrictedExpression::new_set(set_exprs));
        }
    }

    let parents: HashSet<EntityUid> = HashSet::new();
    Entity::new(uid, attrs, parents).expect("Failed to build Cedar principal from claims")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AuthContext;
    use crate::models::Tag;
    use chrono::Utc;
    use std::collections::HashMap as StdHashMap;

    fn make_claims(sub: &str, role: &str) -> JwtClaims {
        let mut extra = StdHashMap::new();
        extra.insert(
            "role".to_string(),
            serde_json::Value::String(role.to_string()),
        );
        extra.insert(
            "groups".to_string(),
            serde_json::Value::Array(vec![serde_json::Value::String(
                "developers".to_string(),
            )]),
        );
        JwtClaims {
            sub: sub.to_string(),
            iss: "test-issuer".to_string(),
            aud: None,
            exp: 9999999999,
            iat: None,
            email: Some(format!("{}@example.com", sub)),
            name: None,
            preferred_username: None,
            extra,
        }
    }

    fn make_asset(
        id: &str,
        created_by: &str,
        auth_ctx: Option<AuthContext>,
        tags: Vec<Tag>,
    ) -> Asset {
        Asset {
            id: id.to_string(),
            title: "Test Asset".to_string(),
            content: None,
            tags,
            metadata: StdHashMap::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: created_by.to_string(),
            updated_by: created_by.to_string(),
            deleted_at: None,
            auth_context: auth_ctx,
        }
    }

    fn make_tag(category: &str, value: &str) -> Tag {
        Tag {
            id: uuid::Uuid::new_v4().to_string(),
            category: category.to_string(),
            value: value.to_string(),
            added_by: "test-user".to_string(),
            added_at: Utc::now(),
        }
    }

    #[test]
    fn test_asset_to_cedar_entity_basic() {
        let asset = make_asset(
            "asset-123",
            "alice@idm.tanbal.ir",
            None,
            vec![make_tag("asset_type", "document")],
        );
        let entity = asset_to_cedar_entity(&asset);
        assert_eq!(entity.uid().to_string(), r#"Asset::"asset-123""#);
    }

    #[test]
    fn test_asset_to_cedar_entity_with_auth_context() {
        let asset = make_asset(
            "asset-456",
            "bob@idm.tanbal.ir",
            Some(AuthContext {
                visibility: "team".to_string(),
                owner_groups: vec!["developers".to_string()],
                confidentiality: "internal".to_string(),
            }),
            vec![
                make_tag("asset_type", "document"),
                make_tag("status", "draft"),
            ],
        );
        let entity = asset_to_cedar_entity(&asset);
        assert_eq!(entity.uid().to_string(), r#"Asset::"asset-456""#);
    }

    #[test]
    fn test_user_to_cedar_principal() {
        let claims = make_claims("alice@idm.tanbal.ir", "admin");
        let entity = user_to_cedar_principal(&claims);
        assert_eq!(
            entity.uid().to_string(),
            r#"User::"alice@idm.tanbal.ir""#
        );
    }
}
