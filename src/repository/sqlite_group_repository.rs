//! SQLite implementation of GroupRepository (workstream 69b4bb4c, task
//! a9f91cfb — PDT-native per-instance groups).

use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::error::{ApiError, Result};
use crate::models::{Group, GroupMember, INSTANCE_ADMINS_GROUP, INSTANCE_ADMINS_GROUP_ID};
use crate::repository::traits::GroupRepository;

pub struct SqliteGroupRepository {
    pool: SqlitePool,
}

impl SqliteGroupRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl GroupRepository for SqliteGroupRepository {
    async fn create_group(&self, name: &str, description: &str, user_id: &str) -> Result<Group> {
        // Precise conflict fast-path (409 naming the group), then an
        // INSERT OR IGNORE so a concurrent create races to a behavioral
        // conflict (rows_affected == 0), never an error-string parse —
        // dialect detail stays inside this impl; the trait contract and
        // the 409 shape are backend-agnostic (postgres: ON CONFLICT DO
        // NOTHING + the same rows_affected check).
        if self.get_group_by_name(name).await?.is_some() {
            return Err(ApiError::Conflict(format!(
                "group '{}' already exists in this instance",
                name
            )));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let result = sqlx::query(
            "INSERT OR IGNORE INTO groups (id, name, description, created_at, created_by)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(name)
        .bind(description)
        .bind(now.to_rfc3339())
        .bind(user_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::Conflict(format!(
                "group '{}' already exists in this instance",
                name
            )));
        }

        Ok(Group {
            id,
            name: name.to_string(),
            description: description.to_string(),
            created_at: now,
            created_by: user_id.to_string(),
        })
    }

    async fn get_group(&self, id: &str) -> Result<Group> {
        let row = sqlx::query_as::<_, GroupRow>(
            "SELECT id, name, description, created_at, created_by FROM groups WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(GroupRow::into_group)
            .ok_or_else(|| ApiError::NotFound(format!("Group {} not found", id)))
    }

    async fn get_group_by_name(&self, name: &str) -> Result<Option<Group>> {
        let row = sqlx::query_as::<_, GroupRow>(
            "SELECT id, name, description, created_at, created_by FROM groups WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(GroupRow::into_group))
    }

    async fn list_groups(&self) -> Result<Vec<Group>> {
        let rows = sqlx::query_as::<_, GroupRow>(
            "SELECT id, name, description, created_at, created_by FROM groups ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(GroupRow::into_group).collect())
    }

    async fn delete_group(&self, id: &str) -> Result<()> {
        // The distinguished instance-admins group is load-bearing (the
        // admin bypass gate) — deletion would silently disarm the bypass.
        if id == INSTANCE_ADMINS_GROUP_ID {
            return Err(ApiError::BadRequest(
                "the distinguished instance-admins group cannot be deleted".to_string(),
            ));
        }
        let result = sqlx::query("DELETE FROM groups WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Group {} not found", id)));
        }
        Ok(())
    }

    async fn add_member(
        &self,
        group_id: &str,
        member_id: &str,
        user_id: &str,
    ) -> Result<GroupMember> {
        // Group must exist (the FK alone would 500 on violation — name it).
        self.get_group(group_id).await?;
        let now = Utc::now();
        sqlx::query(
            "INSERT OR IGNORE INTO group_members (group_id, member_id, added_by, added_at)
             VALUES (?, ?, ?, ?)",
        )
        .bind(group_id)
        .bind(member_id)
        .bind(user_id)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(GroupMember {
            group_id: group_id.to_string(),
            member_id: member_id.to_string(),
            added_by: user_id.to_string(),
            added_at: now,
        })
    }

    async fn remove_member(&self, group_id: &str, member_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM group_members WHERE group_id = ? AND member_id = ?")
            .bind(group_id)
            .bind(member_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_members(&self, group_id: &str) -> Result<Vec<GroupMember>> {
        self.get_group(group_id).await?;
        let rows = sqlx::query_as::<_, MemberRow>(
            "SELECT group_id, member_id, added_by, added_at FROM group_members
             WHERE group_id = ? ORDER BY added_at",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| GroupMember {
                group_id: r.group_id,
                member_id: r.member_id,
                added_by: r.added_by,
                added_at: chrono::DateTime::parse_from_rfc3339(&r.added_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
            .collect())
    }

    async fn memberships_for(&self, member_id: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT g.name FROM groups g
             JOIN group_members m ON m.group_id = g.id
             WHERE m.member_id = ? ORDER BY g.name",
        )
        .bind(member_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows.into_iter().map(|(name,)| name).collect())
    }

    async fn is_instance_admin(&self, member_id: &str) -> Result<bool> {
        // Fail-closed on absence: resolve the distinguished group by its
        // stable NAME (the canonical band reference), then check the
        // membership row. A missing group row → false (no bypass).
        let group = self.get_group_by_name(INSTANCE_ADMINS_GROUP).await?;
        let Some(group) = group else {
            return Ok(false);
        };
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM group_members WHERE group_id = ? AND member_id = ?")
                .bind(&group.id)
                .bind(member_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.is_some())
    }
}

#[derive(sqlx::FromRow)]
struct GroupRow {
    id: String,
    name: String,
    description: Option<String>,
    created_at: String,
    created_by: String,
}

impl GroupRow {
    fn into_group(self) -> Group {
        Group {
            id: self.id,
            name: self.name,
            description: self.description.unwrap_or_default(),
            created_at: chrono::DateTime::parse_from_rfc3339(&self.created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            created_by: self.created_by,
        }
    }
}

#[derive(sqlx::FromRow)]
struct MemberRow {
    group_id: String,
    member_id: String,
    added_by: String,
    added_at: String,
}
