//! MongoDB implementation of GroupRepository (workstream 69b4bb4c, task
//! a9f91cfb — PDT-native per-instance groups).

use bson::doc;
use chrono::Utc;
use futures::TryStreamExt;
use uuid::Uuid;

use crate::db::Database;
use crate::error::{ApiError, Result};
use crate::models::{Group, GroupMember, INSTANCE_ADMINS_GROUP, INSTANCE_ADMINS_GROUP_ID};
use crate::repository::traits::GroupRepository;

/// MongoDB-backed group repository
pub struct MongoGroupRepository {
    db: Database,
}

impl MongoGroupRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
}

#[async_trait::async_trait]
impl GroupRepository for MongoGroupRepository {
    async fn create_group(&self, name: &str, description: &str, user_id: &str) -> Result<Group> {
        // Unique-name enforcement (mongo has no UNIQUE index wired here —
        // check-then-insert; the TOCTOU window is acceptable for the mongo
        // backend, which backs non-tenant stores only).
        if self.get_group_by_name(name).await?.is_some() {
            return Err(ApiError::Conflict(format!(
                "group '{}' already exists in this instance",
                name
            )));
        }
        let group = Group {
            id: Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: description.to_string(),
            created_at: Utc::now(),
            created_by: user_id.to_string(),
        };
        self.db.groups().insert_one(&group).await?;
        Ok(group)
    }

    async fn get_group(&self, id: &str) -> Result<Group> {
        self.db
            .groups()
            .find_one(doc! { "_id": id })
            .await?
            .ok_or_else(|| ApiError::NotFound(format!("Group {} not found", id)))
    }

    async fn get_group_by_name(&self, name: &str) -> Result<Option<Group>> {
        Ok(self.db.groups().find_one(doc! { "name": name }).await?)
    }

    async fn list_groups(&self) -> Result<Vec<Group>> {
        let cursor = self
            .db
            .groups()
            .find(doc! {})
            .with_options(
                mongodb::options::FindOptions::builder()
                    .sort(doc! { "name": 1 })
                    .build(),
            )
            .await?;
        Ok(cursor.try_collect().await?)
    }

    async fn delete_group(&self, id: &str) -> Result<()> {
        if id == INSTANCE_ADMINS_GROUP_ID {
            return Err(ApiError::BadRequest(
                "the distinguished instance-admins group cannot be deleted".to_string(),
            ));
        }
        let result = self.db.groups().delete_one(doc! { "_id": id }).await?;
        if result.deleted_count == 0 {
            return Err(ApiError::NotFound(format!("Group {} not found", id)));
        }
        self.db
            .group_members()
            .delete_many(doc! { "group_id": id })
            .await?;
        Ok(())
    }

    async fn add_member(
        &self,
        group_id: &str,
        member_id: &str,
        user_id: &str,
    ) -> Result<GroupMember> {
        self.get_group(group_id).await?;
        let member = GroupMember {
            group_id: group_id.to_string(),
            member_id: member_id.to_string(),
            added_by: user_id.to_string(),
            added_at: Utc::now(),
        };
        self.db
            .group_members()
            .update_one(
                doc! { "group_id": &member.group_id, "member_id": &member.member_id },
                doc! { "$setOnInsert": bson::to_document(&member)? },
            )
            .upsert(true)
            .await?;
        Ok(member)
    }

    async fn remove_member(&self, group_id: &str, member_id: &str) -> Result<()> {
        self.db
            .group_members()
            .delete_many(doc! { "group_id": group_id, "member_id": member_id })
            .await?;
        Ok(())
    }

    async fn list_members(&self, group_id: &str) -> Result<Vec<GroupMember>> {
        self.get_group(group_id).await?;
        let cursor = self
            .db
            .group_members()
            .find(doc! { "group_id": group_id })
            .await?;
        Ok(cursor.try_collect().await?)
    }

    async fn memberships_for(&self, member_id: &str) -> Result<Vec<String>> {
        let members: Vec<GroupMember> = self
            .db
            .group_members()
            .find(doc! { "member_id": member_id })
            .await?
            .try_collect()
            .await?;
        let mut names = Vec::new();
        for m in members {
            if let Ok(group) = self.get_group(&m.group_id).await {
                names.push(group.name);
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }

    async fn is_instance_admin(&self, member_id: &str) -> Result<bool> {
        let Some(group) = self.get_group_by_name(INSTANCE_ADMINS_GROUP).await? else {
            return Ok(false);
        };
        let row = self
            .db
            .group_members()
            .find_one(doc! { "group_id": &group.id, "member_id": member_id })
            .await?;
        Ok(row.is_some())
    }
}
