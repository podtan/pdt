-- PDT-native per-instance groups (workstream 69b4bb4c, task a9f91cfb —
-- owner directive 2026-11-02 relayed by Farzan).
--
-- PDT speaks INSTANCE, not workspace (owner's correction): each instance
-- store is its own tenancy unit, so these tables are per-instance by
-- construction — the store IS the instance. No new workspace concept.
--
-- All statements are idempotent (IF NOT EXISTS / OR IGNORE): this file is
-- applied at every boot alongside 001_initial.sql.

-- ============================================================================
-- Groups (per-instance)
-- ============================================================================
CREATE TABLE IF NOT EXISTS groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at  TEXT NOT NULL,
    created_by  TEXT NOT NULL
);

-- ============================================================================
-- Group members (per-instance; member_id = the principal's stable id —
-- the token sub for humans/services, the agent id for agents)
-- ============================================================================
CREATE TABLE IF NOT EXISTS group_members (
    group_id   TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    member_id  TEXT NOT NULL,
    added_by   TEXT NOT NULL,
    added_at   TEXT NOT NULL,
    PRIMARY KEY (group_id, member_id)
);

-- Read-time membership resolution is by member id (Phase 1b: effective
-- groups resolve per request) — one index seek per resolve.
CREATE INDEX IF NOT EXISTS idx_group_members_member ON group_members(member_id);

-- ============================================================================
-- The distinguished instance-admins group
--
-- Seeded in EVERY instance store at boot. Members see all assets within
-- this instance (admin bypass, audited) and manage the instance's groups.
-- The fixed UUID is stable across stores; the name is the canonical band
-- reference (bands store group NAMES — membership resolves, not string
-- forms).
-- ============================================================================
INSERT OR IGNORE INTO groups (id, name, description, created_at, created_by)
VALUES (
    '00000000-0000-0000-0000-0000000000a1',
    'instance-admins',
    'Distinguished per-instance admin group: members see all assets within this instance (audited admin bypass) and manage this instance''s groups.',
    strftime('%Y-%m-%dT%H:%M:%SZ', 'now'),
    'system'
);
