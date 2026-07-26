-- SQLite schema for PDT
-- All tables use TEXT for UUIDs (SQLite has no native UUID type)
-- Datetimes are stored as ISO8601 TEXT strings

-- ============================================================================
-- Assets
-- ============================================================================
CREATE TABLE IF NOT EXISTS assets (
    id              TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    content         TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    updated_by      TEXT NOT NULL,
    deleted_at      TEXT,
    -- Flattened auth_context (nullable — absent means grandfathered)
    visibility      TEXT,
    confidentiality TEXT,
    owner_groups    TEXT  -- JSON array as text, e.g. ["dev","ops"]
);

CREATE INDEX IF NOT EXISTS idx_assets_created_at ON assets(created_at);
CREATE INDEX IF NOT EXISTS idx_assets_updated_at ON assets(updated_at);
CREATE INDEX IF NOT EXISTS idx_assets_deleted_at ON assets(deleted_at);
CREATE INDEX IF NOT EXISTS idx_assets_created_by ON assets(created_by);

-- ============================================================================
-- Tags (normalized — one row per tag)
-- ============================================================================
CREATE TABLE IF NOT EXISTS tags (
    id          TEXT PRIMARY KEY,
    asset_id    TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    category    TEXT NOT NULL,
    value       TEXT NOT NULL,
    added_by    TEXT NOT NULL,
    added_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_tags_asset_id ON tags(asset_id);
CREATE INDEX IF NOT EXISTS idx_tags_category_value ON tags(category, value);

-- ============================================================================
-- Asset Metadata (key-value pairs)
-- ============================================================================
CREATE TABLE IF NOT EXISTS asset_metadata (
    asset_id    TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    key         TEXT NOT NULL,
    value_json  TEXT NOT NULL,
    PRIMARY KEY (asset_id, key)
);

-- ============================================================================
-- Relations
-- ============================================================================
CREATE TABLE IF NOT EXISTS relations (
    id              TEXT PRIMARY KEY,
    from_asset_id   TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    to_asset_id     TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    relation_type   TEXT NOT NULL,
    metadata_json   TEXT NOT NULL DEFAULT '{}',
    created_at      TEXT NOT NULL,
    created_by      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_relations_from_asset ON relations(from_asset_id);
CREATE INDEX IF NOT EXISTS idx_relations_to_asset ON relations(to_asset_id);
CREATE INDEX IF NOT EXISTS idx_relations_type ON relations(relation_type);

-- ============================================================================
-- Collections
-- ============================================================================
CREATE TABLE IF NOT EXISTS collections (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    description     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    created_by      TEXT NOT NULL,
    updated_by      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_collections_name ON collections(name);

-- Collection tags (same structure as asset tags)
CREATE TABLE IF NOT EXISTS collection_tags (
    id              TEXT PRIMARY KEY,
    collection_id   TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    category        TEXT NOT NULL,
    value           TEXT NOT NULL,
    added_by        TEXT NOT NULL,
    added_at        TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_collection_tags_collection_id ON collection_tags(collection_id);

-- Junction table: collection <-> asset
CREATE TABLE IF NOT EXISTS collection_assets (
    collection_id   TEXT NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    asset_id        TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    PRIMARY KEY (collection_id, asset_id)
);

CREATE INDEX IF NOT EXISTS idx_collection_assets_asset ON collection_assets(asset_id);

-- ============================================================================
-- Audit Entries
-- ============================================================================
CREATE TABLE IF NOT EXISTS audit_entries (
    id              TEXT PRIMARY KEY,
    entity_type     TEXT NOT NULL,
    entity_id       TEXT NOT NULL,
    action          TEXT NOT NULL,
    changes_json    TEXT NOT NULL DEFAULT '{}',
    user_id         TEXT NOT NULL,
    timestamp       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_audit_entity ON audit_entries(entity_type, entity_id);
CREATE INDEX IF NOT EXISTS idx_audit_user ON audit_entries(user_id);
CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_entries(timestamp DESC);

-- ============================================================================
-- Full-Text Search (FTS5)
-- ============================================================================
CREATE VIRTUAL TABLE IF NOT EXISTS assets_fts USING fts5(
    title,
    content,
    content='assets',
    content_rowid='rowid'
);

-- Triggers to keep FTS index in sync with assets table
CREATE TRIGGER IF NOT EXISTS assets_ai AFTER INSERT ON assets BEGIN
    INSERT INTO assets_fts(rowid, title, content)
    VALUES (new.rowid, new.title, new.content);
END;

CREATE TRIGGER IF NOT EXISTS assets_ad AFTER DELETE ON assets BEGIN
    INSERT INTO assets_fts(assets_fts, rowid, title, content)
    VALUES ('delete', old.rowid, old.title, old.content);
END;

CREATE TRIGGER IF NOT EXISTS assets_au AFTER UPDATE ON assets BEGIN
    INSERT INTO assets_fts(assets_fts, rowid, title, content)
    VALUES ('delete', old.rowid, old.title, old.content);
    INSERT INTO assets_fts(rowid, title, content)
    VALUES (new.rowid, new.title, new.content);
END;
