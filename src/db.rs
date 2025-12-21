//! Database connection and operations

use anyhow::Result;
use mongodb::{
    options::{ClientOptions, Credential},
    Client, Collection, Database as MongoDatabase, IndexModel,
};

use crate::config::DatabaseConfig;
use crate::models::{Asset, AuditEntry, Collection as CollectionModel, Relation};

/// Database wrapper for MongoDB/DocumentDB operations
#[derive(Clone)]
pub struct Database {
    db: MongoDatabase,
}

impl Database {
    /// Connect to the database
    pub async fn connect(config: &DatabaseConfig) -> Result<Self> {
        let mut options = ClientOptions::parse(config.connection_string()).await?;

        // Set credentials
        options.credential = Some(
            Credential::builder()
                .username(config.username.clone())
                .password(config.password.clone())
                .source("admin".to_string())
                .build(),
        );

        let client = Client::with_options(options)?;

        // Verify connection
        client
            .database("admin")
            .run_command(bson::doc! { "ping": 1 })
            .await?;

        let db = client.database(&config.database);

        Ok(Database { db })
    }

    /// Get the assets collection
    pub fn assets(&self) -> Collection<Asset> {
        self.db.collection("assets")
    }

    /// Get the relations collection
    pub fn relations(&self) -> Collection<Relation> {
        self.db.collection("relations")
    }

    /// Get the collections collection
    pub fn collections(&self) -> Collection<CollectionModel> {
        self.db.collection("collections")
    }

    /// Get the audit entries collection
    pub fn audit(&self) -> Collection<AuditEntry> {
        self.db.collection("audit")
    }

    /// Create database indices
    pub async fn create_indices(&self) -> Result<()> {
        // Asset indices
        let asset_indices = vec![
            IndexModel::builder()
                .keys(bson::doc! { "tags.category": 1, "tags.value": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "created_by": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "deleted_at": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "title": "text", "content": "text" })
                .build(),
        ];
        self.assets().create_indexes(asset_indices).await?;

        // Relation indices
        let relation_indices = vec![
            IndexModel::builder()
                .keys(bson::doc! { "from_asset_id": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "to_asset_id": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "relation_type": 1 })
                .build(),
        ];
        self.relations().create_indexes(relation_indices).await?;

        // Collection indices
        let collection_indices = vec![
            IndexModel::builder().keys(bson::doc! { "name": 1 }).build(),
            IndexModel::builder()
                .keys(bson::doc! { "asset_ids": 1 })
                .build(),
        ];
        self.collections()
            .create_indexes(collection_indices)
            .await?;

        // Audit indices
        let audit_indices = vec![
            IndexModel::builder()
                .keys(bson::doc! { "entity_type": 1, "entity_id": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "user_id": 1 })
                .build(),
            IndexModel::builder()
                .keys(bson::doc! { "timestamp": -1 })
                .build(),
        ];
        self.audit().create_indexes(audit_indices).await?;

        Ok(())
    }
}
