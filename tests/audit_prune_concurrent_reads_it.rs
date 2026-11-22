//! Audit-prune UNDER CONCURRENT LOAD — the P1 incident regression guard
//! (issue 57bab0a9, incident 2026-11-22: the single-statement prune delete
//! held the leaf write lock for the whole delete; the 3-connection
//! per-instance pool saturated behind it — blocked writers holding
//! connections for the 5s busy_timeout — and the THQ console read path
//! starved: blank console until a PDT restart).
//!
//! THE GUARD (CPO ask 4, platform law — every fix ships with a guard): a
//! FULL execution prune (archive walk + delete) runs while reader loops
//! (the console read class: pure SELECTs) and writer loops (the
//! reconciler/report/audit-riding write class: audit INSERTs) hammer the
//! SAME instance-shaped pool. With the chunked delete (500-row PK chunks,
//! commit + yield per chunk) the write lock is never held long enough to
//! block a writer past its busy_timeout or starve a reader:
//!   - every reader read succeeds,
//!   - every writer write succeeds (zero SQLITE_BUSY),
//!   - reader wait stays bounded (no multi-second pool starvation),
//!   - the prune still verifies end-to-end (deleted == archived,
//!     post-count 0, fresh rows survive).
//!
//! The pool mirrors the per-instance shape (tenant.rs): FILE db, WAL,
//! max_connections = 3, sqlx defaults otherwise.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdt::repository::{
    AuditRepository, SqliteAssetRepository, SqliteAuditRepository, SqliteCollectionRepository,
    SqliteGroupRepository, SqliteRelationRepository,
};
use pdt::service::{AuditService, Services};

const INSTANCE_POOL_MAX_CONN: u32 = 3; // tenant.rs:47 — the incident shape

async fn instance_shape_services(
    db_path: &std::path::Path,
) -> (Services, sqlx::sqlite::SqlitePool) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(INSTANCE_POOL_MAX_CONN)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(db_path)
                .create_if_missing(true)
                .foreign_keys(true)
                .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
        )
        .await
        .expect("instance-shaped pool connects");

    let migration_sql = include_str!("../migrations/sqlite/001_initial.sql");
    sqlx::raw_sql(migration_sql)
        .execute(&pool)
        .await
        .expect("migrations");

    let services = Services::from_repositories(
        SqliteAssetRepository::new(pool.clone()),
        SqliteRelationRepository::new(pool.clone()),
        SqliteCollectionRepository::new(pool.clone()),
        SqliteAuditRepository::new(pool.clone()),
        SqliteGroupRepository::new(pool.clone()),
    );
    (services, pool)
}

/// Seed `count` backdated audit rows with ~1KB payloads (prod-like row
/// shape), batched in transactions of 1000 for seed speed.
async fn seed_old_entries(pool: &sqlx::sqlite::SqlitePool, count: usize) {
    let payload = "x".repeat(1024);
    let old_ts = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
    let mut seeded = 0usize;
    while seeded < count {
        let mut tx = pool.begin().await.expect("seed tx");
        for i in seeded..seeded + 1000 {
            sqlx::query(
                "INSERT INTO audit_entries (id, entity_type, entity_id, action, changes_json, user_id, timestamp)
                 VALUES (?, 'asset', ?, 'update', ?, 'seed-old', ?)",
            )
            .bind(format!("00000000-0000-4000-8000-{i:012}"))
            .bind(format!("old-entity-{i}"))
            .bind(format!("{{\"changes\":\"{payload}\"}}"))
            .bind(&old_ts)
            .execute(&mut *tx)
            .await
            .expect("seed insert");
        }
        tx.commit().await.expect("seed commit");
        seeded += 1000;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn full_prune_under_concurrent_reads_and_writes_never_blocks_the_read_path() {
    let dir =
        std::env::temp_dir().join(format!("pdt_prune_concurrent_it_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("tmpdir");
    let db_path = dir.join("leaf.db");
    let archive_dir = dir.join("archive");

    let (services, pool) = instance_shape_services(&db_path).await;
    seed_old_entries(&pool, 6_000).await;

    let stop = Arc::new(AtomicBool::new(false));
    let reader_ok = Arc::new(AtomicUsize::new(0));
    let reader_err = Arc::new(AtomicUsize::new(0));
    let reader_max_wait_ms = Arc::new(AtomicUsize::new(0));
    let writer_ok = Arc::new(AtomicUsize::new(0));
    let writer_err = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();

    // 3 reader loops — the console read class (pure SELECTs, no audit on
    // reads): count_before through the SAME pool the prune uses.
    for _ in 0..3 {
        let stop = stop.clone();
        let ok = reader_ok.clone();
        let err = reader_err.clone();
        let max_wait = reader_max_wait_ms.clone();
        let audit = SqliteAuditRepository::new(pool.clone());
        handles.push(tokio::spawn(async move {
            while !stop.load(Ordering::Relaxed) {
                let started = Instant::now();
                match audit
                    .count_before(chrono::Utc::now() - chrono::Duration::days(31))
                    .await
                {
                    Ok(_) => {
                        ok.fetch_add(1, Ordering::Relaxed);
                        max_wait
                            .fetch_max(started.elapsed().as_millis() as usize, Ordering::Relaxed);
                    }
                    Err(e) => {
                        err.fetch_add(1, Ordering::Relaxed);
                        panic!(
                            "READER FAILED during prune after {:?}: {e}",
                            started.elapsed()
                        );
                    }
                }
            }
        }));
    }

    // 2 writer loops — the reconciler/report/audit-riding write class:
    // audit INSERTs (the exact call every PDT write op makes inline).
    for w in 0..2 {
        let stop = stop.clone();
        let ok = writer_ok.clone();
        let err = writer_err.clone();
        let audit = SqliteAuditRepository::new(pool.clone());
        handles.push(tokio::spawn(async move {
            let mut n = 0u64;
            while !stop.load(Ordering::Relaxed) {
                match audit
                    .create(
                        "asset",
                        &format!("writer-entity-{w}-{n}"),
                        pdt::models::AuditAction::Update,
                        serde_json::json!({"loop": w, "n": n}),
                        "writer-loop",
                    )
                    .await
                {
                    Ok(_) => {
                        ok.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        err.fetch_add(1, Ordering::Relaxed);
                        panic!("WRITER FAILED during prune (SQLITE_BUSY class): {e}");
                    }
                }
                n += 1;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }));
    }

    // THE FULL EXECUTION PRUNE — the exact production mechanics
    // (archive walk + row-count verify + chunked delete + post-verify).
    let report = AuditService::audit_prune(
        &services,
        31,
        false,
        "incident-guard-test",
        Some(archive_dir.clone()),
    )
    .await
    .expect("execution prune completes");

    // Let the loops observe the post-prune window briefly, then stop.
    tokio::time::sleep(Duration::from_millis(300)).await;
    stop.store(true, Ordering::Relaxed);
    for h in handles {
        h.await.expect("loops join");
    }

    // Prune verified end-to-end.
    assert_eq!(report["dry_run"], serde_json::json!(false));
    assert_eq!(report["candidates"], serde_json::json!(6000));
    assert_eq!(report["archived"], serde_json::json!(6000));
    assert_eq!(report["deleted"], serde_json::json!(6000));
    assert_eq!(report["post_count"], serde_json::json!(0));
    assert_eq!(report["verified"], serde_json::json!(true));

    // THE GUARD ASSERTIONS — the read path never starves, the write path
    // never fails, under a FULL prune at incident shape.
    let r_ok = reader_ok.load(Ordering::Relaxed);
    let r_err = reader_err.load(Ordering::Relaxed);
    let r_max = reader_max_wait_ms.load(Ordering::Relaxed);
    let w_ok = writer_ok.load(Ordering::Relaxed);
    let w_err = writer_err.load(Ordering::Relaxed);
    assert!(r_ok > 0, "readers must have run");
    assert!(w_ok > 0, "writers must have run");
    assert_eq!(r_err, 0, "zero reader failures during the prune");
    assert_eq!(
        w_err, 0,
        "zero writer failures (SQLITE_BUSY class) during the prune"
    );
    assert!(
        r_max < 4_000,
        "reader max wait {r_max}ms must stay well under the 5s busy_timeout / 30s \
         acquire_timeout — a multi-second wait is the starvation signature (the \
         single-statement delete held the lock for the whole delete; the chunked \
         delete holds it for milliseconds per chunk)"
    );

    // Fresh rows written DURING the prune survive (writers were real).
    let surviving = services
        .audit()
        .count_before(chrono::Utc::now())
        .await
        .expect("count");
    assert!(
        surviving >= w_ok as i64,
        "the rows the writer loops wrote during the prune must survive: surviving={surviving} writer_ok={w_ok}"
    );

    // The archive holds every deleted row (the record is complete).
    let archive_file = std::fs::read_dir(&archive_dir)
        .expect("archive dir reads")
        .filter_map(|e| e.ok())
        .find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("audit-archive-")
        })
        .expect("archive file exists");
    let lines = std::fs::read_to_string(archive_file.path())
        .expect("archive reads")
        .lines()
        .count();
    assert_eq!(
        lines, 6000,
        "archive manifest count == deleted count (ask 3b shape)"
    );

    pool.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}
