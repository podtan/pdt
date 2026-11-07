//! Audit service

use crate::error::{ApiError, Result};
use crate::models::{AuditAction, AuditEntry};
use crate::service::Services;
use chrono::Utc;

/// Service for audit business logic
pub struct AuditService;

impl AuditService {
    /// List audit entries with filters
    pub async fn list(
        services: &Services,
        entity_type: Option<&str>,
        entity_id: Option<&str>,
        user_id: Option<&str>,
        limit: i64,
        cursor: Option<&str>,
    ) -> Result<(Vec<AuditEntry>, Option<String>)> {
        services
            .audit()
            .list(entity_type, entity_id, user_id, limit, cursor)
            .await
    }

    /// Get history for a specific asset
    pub async fn get_asset_history(
        services: &Services,
        asset_id: &str,
        limit: i64,
    ) -> Result<Vec<AuditEntry>> {
        services
            .audit()
            .get_entity_history("asset", asset_id, limit)
            .await
    }

    /// Count audit entries older than the cutoff (the audit-prune candidate
    /// count, issue 2c71e4e6).
    pub async fn count_before(services: &Services, cutoff: chrono::DateTime<Utc>) -> Result<i64> {
        services.audit().count_before(cutoff).await
    }

    /// AUDIT-ENTRIES RETENTION PRUNE (issue 2c71e4e6 — the THQ band's
    /// 1.88M-row audit_entries churn): ARCHIVE-THEN-DELETE, the mechanics
    /// behind POST /api/maintenance/audit-prune.
    ///
    /// BINDING GUARDS, mechanically enforced:
    /// 1. DRY-RUN ZERO-WRITES: dry_run=true answers the numbers only — no
    ///    file, no delete, no audit entry.
    /// 2. ARCHIVE-FIRST: execution exports EVERY candidate row to a dated
    ///    JSONL archive ({archive_dir}/audit-archive-<ts>.jsonl) and fsyncs
    ///    it BEFORE any delete — the archive IS the record.
    /// 3. ROW-COUNT VERIFY: the delete is REFUSED unless the archived row
    ///    count equals the candidate count (a partial archive is removed,
    ///    never kept, never trusted).
    /// 4. NO SILENT DELETES: the delete's rows_affected and the post-delete
    ///    count are both in the report; the prune itself rides ONE audit
    ///    entry (action=delete, entity_type=audit_entries) carrying the
    ///    archive path + counts.
    ///
    /// The gates (admin / PDT_PRUNE_ENABLED / policy window) live in the
    /// HANDLER — this function is the mechanics, callable only after the
    /// handler cleared them (defense-in-depth: it re-checks nothing about
    /// policy; the handler is the only caller).
    pub async fn audit_prune(
        services: &Services,
        older_than_days: u32,
        dry_run: bool,
        user_id: &str,
        archive_dir: Option<std::path::PathBuf>,
    ) -> Result<serde_json::Value> {
        let cutoff = Utc::now() - chrono::Duration::days(older_than_days as i64);
        let candidates = services.audit().count_before(cutoff).await?;

        if dry_run {
            // Dry-run: the numbers only. Zero writes, zero audits. A sample
            // of the newest candidates rides the report (the CPO sight reads
            // what WOULD be archived, not just how many).
            let (sample, _) = services.audit().list_before(cutoff, 10, None).await?;
            let listed: Vec<serde_json::Value> = sample
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "entity_type": e.entity_type,
                        "entity_id": e.entity_id,
                        "action": e.action.to_string(),
                        "timestamp": e.timestamp.to_rfc3339(),
                    })
                })
                .collect();
            return Ok(serde_json::json!({
                "dry_run": true,
                "candidates": candidates,
                "older_than_days": older_than_days,
                "cutoff": cutoff.to_rfc3339(),
                "sample": listed,
                "archive_dir": archive_dir
                    .map(|d| d.to_string_lossy().to_string()),
            }));
        }

        // Execution — the archive directory is REQUIRED (the archive is the
        // record; no archive, no delete).
        let archive_dir = archive_dir.ok_or_else(|| {
            ApiError::Internal(
                "audit archive directory unresolved — execution refused (the archive is the \
                 record; no archive, no delete)"
                    .to_string(),
            )
        })?;
        std::fs::create_dir_all(&archive_dir).map_err(|e| {
            ApiError::Internal(format!(
                "audit archive directory not creatable ({}): {e} — execution refused",
                archive_dir.display()
            ))
        })?;

        // ARCHIVE phase: cursor-walk every candidate row (id-ASC pagination —
        // exact, no skips) into a dated JSONL file, one AuditEntry per line.
        let archive_path = archive_dir.join(format!(
            "audit-archive-{}.jsonl",
            Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        let file = std::fs::File::create(&archive_path)
            .map_err(|e| ApiError::Internal(format!("archive file not creatable: {e}")))?;
        let mut writer = std::io::BufWriter::new(file);
        let mut archived: u64 = 0;
        let mut cursor: Option<String> = None;
        loop {
            let (page, next) = services
                .audit()
                .list_before(cutoff, 500, cursor.as_deref())
                .await?;
            for entry in &page {
                serde_json::to_writer(&mut writer, entry)
                    .map_err(|e| ApiError::Internal(format!("archive row write failed: {e}")))?;
                std::io::Write::write_all(&mut writer, b"\n")
                    .map_err(|e| ApiError::Internal(format!("archive row write failed: {e}")))?;
                archived += 1;
            }
            match next {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }
        std::io::Write::flush(&mut writer)
            .map_err(|e| ApiError::Internal(format!("archive flush failed: {e}")))?;
        // fsync — the archive must be durable on disk BEFORE any delete.
        writer
            .into_inner()
            .map_err(|e| ApiError::Internal(format!("archive flush failed: {e}")))?
            .sync_all()
            .map_err(|e| ApiError::Internal(format!("archive fsync failed: {e}")))?;

        // ROW-COUNT VERIFY: the delete is refused unless the archive holds
        // EVERY candidate. A partial archive is REMOVED (never kept, never
        // trusted) and the delete never fires.
        if archived != candidates as u64 {
            let _ = std::fs::remove_file(&archive_path);
            return Err(ApiError::Internal(format!(
                "archive verify failed: wrote {archived} rows, expected {candidates} — partial \
                 archive removed, delete REFUSED (no silent deletes)"
            )));
        }

        // DELETE phase: one statement; rows_affected is the deleted count.
        let deleted = services.audit().delete_before(cutoff).await?;

        // POST-VERIFY: nothing older than the cutoff remains.
        let post_count = services.audit().count_before(cutoff).await?;
        let verified = post_count == 0 && deleted == archived;

        // The prune itself rides ONE audit entry (timestamped NOW — it
        // survives its own prune window).
        services
            .audit()
            .create(
                "audit_entries",
                "*",
                AuditAction::Delete,
                serde_json::json!({
                    "operation": "audit-prune",
                    "older_than_days": older_than_days,
                    "cutoff": cutoff.to_rfc3339(),
                    "archived": archived,
                    "deleted": deleted,
                    "archive_path": archive_path.to_string_lossy(),
                }),
                user_id,
            )
            .await?;

        Ok(serde_json::json!({
            "dry_run": false,
            "candidates": candidates,
            "archived": archived,
            "deleted": deleted,
            "post_count": post_count,
            "verified": verified,
            "archive_path": archive_path.to_string_lossy(),
            "older_than_days": older_than_days,
            "cutoff": cutoff.to_rfc3339(),
        }))
    }
}
