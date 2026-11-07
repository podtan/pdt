//! Prune retention policy — class windows + dispositions loaded from DATA.
//!
//! CPO precondition 2 (routing 0ae73f46; policy of record = TanbalCo/infra
//! `runbooks/pdt-retention-proposal.md` @ commit b78efcd, endorsed CPO ruling
//! 8d591e85). The prune endpoint takes a raw (tag_category, tag_value,
//! older_than_days) — the policy table maps each class to its window and
//! disposition so a raw call cannot soft-delete a class the policy says is
//! archive-only. The hole this closes (paydar dry-run finding, 2026-10-26):
//! a raw (type,message,14d) call matched 199 live channel messages — the
//! messages-of-record class is archive-only (180d → archive, never
//! silent-delete) and must be unrepresentable at the wire.
//!
//! DATA, NOT CODE: the table loads from a JSON file at PDT_PRUNE_POLICY_PATH
//! at INVOCATION (the operator can correct the file without a restart; the
//! endpoint never caches policy data). No policy file / unreadable /
//! unparsable = no policy = execution refused for every class (fail-closed);
//! dry-run still answers — the numbers are the gate's input.
//!
//! Canonical entries from the endorsed proposal (the operator's policy file
//! encodes these verbatim):
//! - audit trail:        90d hot → archive (never delete)
//! - operational noise:  14d → soft-delete
//! - drill artifacts:    30d → soft-delete (supersession-aware — see below)
//! - messages of record: 180d → archive (never silent-delete)
//! - assets of record:   indefinite (explicit v+1 supersession only)
//!
//! The AUDIT-ENTRIES TABLE class (issue 2c71e4e6 — the THQ band's 1.88M-row
//! audit churn) is a TABLE class, not a (tag_category, tag_value) asset
//! class: it rides the dedicated `audit_entries` policy section + the
//! POST /api/maintenance/audit-prune verb. Its endorsed disposition is
//! ARCHIVE-THEN-DELETE (90d hot → dated archive file → delete): the archive
//! IS the record, so the delete is not a silent delete — but the disposition
//! must be DECLARED in the policy (absent section = execution refused,
//! fail-closed). The asset-prune verb still refuses the class (it cannot
//! touch the audit_entries table).
//!
//! Supersession-awareness (drill artifacts) is a candidate-selection
//! refinement, not a window guard: the first execution class is noise
//! (rahnama routing 21894fd2), and the drill class must not execute until
//! the supersession guard lands — recorded in the deploy brief.

use serde::Deserialize;

/// Disposition of a class under the retention policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Archive at the window — NEVER soft-delete (messages of record 180d,
    /// audit trail 90d). Execution refused for the class, always.
    Archive,
    /// Soft-delete at the window (operational noise 14d, drill artifacts 30d).
    SoftDelete,
    /// Archive-then-delete: rows are exported to a dated archive file with
    /// row-count verify FIRST, then deleted — the archive IS the record, so
    /// the delete is not silent. The audit-entries table class (issue
    /// 2c71e4e6); executable ONLY via the audit-prune verb, never the asset
    /// prune.
    ArchiveThenDelete,
}

/// One class entry — the (tag_category, tag_value) pair identifies the class
/// on the wire; window_days + disposition are the policy of record.
#[derive(Debug, Clone, Deserialize)]
pub struct ClassEntry {
    pub tag_category: String,
    pub tag_value: String,
    pub window_days: u32,
    pub disposition: Disposition,
}

/// The AUDIT-ENTRIES TABLE class (issue 2c71e4e6) — a dedicated policy
/// section, not a (tag_category, tag_value) asset class: the audit_entries
/// table is not an asset and carries no tags. Absent section = the class
/// has no retention policy = execution refused (fail-closed).
#[derive(Debug, Clone, Deserialize)]
pub struct AuditEntriesClass {
    pub window_days: u32,
    pub disposition: Disposition,
}

/// The retention policy — a list of class entries loaded from the policy
/// file. Duplicate (category, value) pairs: FIRST entry wins (the operator's
/// file is authoritative; the loader never widens on duplicates).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PrunePolicy {
    #[serde(default)]
    classes: Vec<ClassEntry>,
    #[serde(default)]
    audit_entries: Option<AuditEntriesClass>,
}

/// Verdict for an execution attempt against the policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PruneVerdict {
    /// Class is policy-cleared for soft-delete at this window.
    Allowed,
    /// Refused — the reason is the 403 body (archive-only class, window
    /// below the policy window, or class absent from the policy).
    Refused(String),
}

impl PrunePolicy {
    /// Load the policy from an explicit path (env-injected for testability).
    /// Any failure — unset, unreadable, unparsable — is None (fail-closed).
    pub fn load(path: Option<&std::ffi::OsStr>) -> Option<PrunePolicy> {
        let path = path?;
        let raw = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Load from PDT_PRUNE_POLICY_PATH — read AT INVOCATION.
    pub fn load_from_env() -> Option<PrunePolicy> {
        Self::load(std::env::var_os("PDT_PRUNE_POLICY_PATH").as_deref())
    }

    /// Evaluate an execution attempt for a class + window against the policy.
    ///
    /// Guards, in order (fail-closed at every step):
    /// 1. Class absent from the policy → refused (an unclassified class has
    ///    no retention policy — never prune it by window).
    /// 2. Disposition archive → refused (soft-delete of an archive-only
    ///    class is the silent-delete the policy forbids).
    /// 3. Window below the policy window → refused (a call pruning MORE
    ///    aggressively than policy — only prune at least as old as policy).
    pub fn evaluate(
        &self,
        tag_category: &str,
        tag_value: &str,
        older_than_days: u32,
    ) -> PruneVerdict {
        let entry = match self
            .classes
            .iter()
            .find(|c| c.tag_category == tag_category && c.tag_value == tag_value)
        {
            Some(e) => e,
            None => {
                return PruneVerdict::Refused(format!(
                    "class ({tag_category},{tag_value}) is not in the retention policy — \
                     execution refused (fail-closed; dry-run remains available)"
                ))
            }
        };
        match entry.disposition {
            Disposition::Archive => PruneVerdict::Refused(format!(
                "class ({tag_category},{tag_value}) is archive-only under the retention policy \
                 (window {}d → archive, never delete) — execution refused",
                entry.window_days
            )),
            Disposition::SoftDelete => {
                if older_than_days < entry.window_days {
                    PruneVerdict::Refused(format!(
                        "window {older_than_days}d is below the policy window ({}d) for class \
                         ({tag_category},{tag_value}) — execution refused (only prune at least \
                         as old as policy)",
                        entry.window_days
                    ))
                } else {
                    PruneVerdict::Allowed
                }
            }
            // The asset prune is soft-delete-only (archive-over-delete hard
            // line): an archive-then-delete class is not executable HERE —
            // the audit-entries verb is the only archive-then-delete executor.
            Disposition::ArchiveThenDelete => PruneVerdict::Refused(format!(
                "class ({tag_category},{tag_value}) disposition archive_then_delete is not \
                 executable via the asset prune (soft-delete only) — the audit-entries verb \
                 is the only archive-then-delete executor",
            )),
        }
    }

    /// Evaluate an audit-prune execution attempt (the audit_entries TABLE
    /// class, issue 2c71e4e6) against the policy.
    ///
    /// Guards, in order (fail-closed at every step):
    /// 1. Section absent from the policy → refused (the audit-entries class
    ///    has no retention policy — never prune it by window).
    /// 2. Disposition archive or soft_delete → refused (the audit trail is
    ///    the record: archive-only is unexecutable by this verb, and bare
    ///    soft-delete of audit rows is prohibited — the endorsed disposition
    ///    is archive_then_delete, declared in the policy).
    /// 3. Window below the policy window → refused (only prune at least as
    ///    old as policy).
    pub fn evaluate_audit_entries(&self, older_than_days: u32) -> PruneVerdict {
        let entry = match &self.audit_entries {
            Some(e) => e,
            None => {
                return PruneVerdict::Refused(
                    "no audit-entries class in the retention policy (PDT_PRUNE_POLICY_PATH) — \
                     execution refused (fail-closed; dry-run remains available)"
                        .to_string(),
                )
            }
        };
        match entry.disposition {
            Disposition::Archive => PruneVerdict::Refused(
                "audit-entries class is archive-only under the retention policy — this verb \
                 archives-then-deletes, so the policy must carry disposition \
                 archive_then_delete"
                    .to_string(),
            ),
            Disposition::SoftDelete => PruneVerdict::Refused(
                "audit-entries soft-delete is prohibited (the audit trail is the record) — \
                 the policy must carry disposition archive_then_delete"
                    .to_string(),
            ),
            Disposition::ArchiveThenDelete => {
                if older_than_days < entry.window_days {
                    PruneVerdict::Refused(format!(
                        "window {older_than_days}d is below the policy window ({}d) for the \
                         audit-entries class — execution refused (only prune at least as old \
                         as policy)",
                        entry.window_days
                    ))
                } else {
                    PruneVerdict::Allowed
                }
            }
        }
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;

    /// The endorsed proposal table, verbatim (runbooks/pdt-retention-proposal.md
    /// @ b78efcd) — the shape the operator's policy file encodes.
    const ENDORSED: &str = r#"{
        "classes": [
            {"tag_category": "type", "tag_value": "audit",   "window_days": 90,  "disposition": "archive"},
            {"tag_category": "type", "tag_value": "noise",   "window_days": 14,  "disposition": "soft_delete"},
            {"tag_category": "type", "tag_value": "drill",   "window_days": 30,  "disposition": "soft_delete"},
            {"tag_category": "type", "tag_value": "message", "window_days": 180, "disposition": "archive"}
        ]
    }"#;

    fn endorsed() -> PrunePolicy {
        serde_json::from_str(ENDORSED).expect("endorsed policy parses")
    }

    #[test]
    fn endorsed_table_parses_with_all_four_classes() {
        let p = endorsed();
        assert_eq!(p.classes.len(), 4);
    }

    #[test]
    fn message_class_is_archive_only_execution_refused() {
        // THE paydar dry-run finding pin: a raw (type,message,14d) call must
        // never soft-delete messages of record (180d → archive).
        let p = endorsed();
        match p.evaluate("type", "message", 14) {
            PruneVerdict::Refused(reason) => {
                assert!(reason.contains("archive-only"), "{reason}");
            }
            PruneVerdict::Allowed => panic!("message class must be refused — archive-only"),
        }
    }

    #[test]
    fn message_class_refused_even_at_policy_window() {
        let p = endorsed();
        assert!(matches!(
            p.evaluate("type", "message", 180),
            PruneVerdict::Refused(_)
        ));
        assert!(matches!(
            p.evaluate("type", "message", 365),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn audit_class_is_archive_only_execution_refused() {
        let p = endorsed();
        assert!(matches!(
            p.evaluate("type", "audit", 90),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn noise_class_allowed_at_and_above_policy_window() {
        let p = endorsed();
        assert_eq!(p.evaluate("type", "noise", 14), PruneVerdict::Allowed);
        assert_eq!(p.evaluate("type", "noise", 30), PruneVerdict::Allowed);
    }

    #[test]
    fn noise_class_refused_below_policy_window() {
        // A call pruning MORE aggressively than policy (7d < 14d) is refused.
        let p = endorsed();
        match p.evaluate("type", "noise", 7) {
            PruneVerdict::Refused(reason) => {
                assert!(reason.contains("below the policy window"), "{reason}");
            }
            PruneVerdict::Allowed => panic!("sub-policy window must be refused"),
        }
    }

    #[test]
    fn unlisted_class_refused_fail_closed() {
        let p = endorsed();
        assert!(matches!(
            p.evaluate("type", "report", 400),
            PruneVerdict::Refused(_)
        ));
        assert!(matches!(
            p.evaluate("anything", "else", 400),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn empty_policy_refuses_every_class() {
        let p = PrunePolicy::default();
        assert!(matches!(
            p.evaluate("type", "noise", 14),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn load_none_path_is_none() {
        assert!(PrunePolicy::load(None).is_none());
    }

    #[test]
    fn load_missing_file_is_none() {
        assert!(PrunePolicy::load(Some("/nonexistent/prune-policy.json".as_ref())).is_none());
    }

    #[test]
    fn load_malformed_file_is_none_fail_closed() {
        let dir = std::env::temp_dir().join("pdt-prune-policy-test-malformed");
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("policy.json");
        std::fs::write(&path, "{not json").expect("write malformed");
        assert!(PrunePolicy::load(Some(path.as_os_str())).is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_endorsed_file_round_trips() {
        let dir = std::env::temp_dir().join("pdt-prune-policy-test-endorsed");
        std::fs::create_dir_all(&dir).expect("tmpdir");
        let path = dir.join("policy.json");
        std::fs::write(&path, ENDORSED).expect("write endorsed");
        let p = PrunePolicy::load(Some(path.as_os_str())).expect("endorsed file loads");
        assert_eq!(p.evaluate("type", "message", 14).clone(), {
            match p.evaluate("type", "message", 14) {
                v @ PruneVerdict::Refused(_) => v,
                PruneVerdict::Allowed => panic!("message must refuse"),
            }
        });
        assert_eq!(p.evaluate("type", "noise", 14), PruneVerdict::Allowed);
        let _ = std::fs::remove_file(&path);
    }

    // ---- The audit-entries TABLE class (issue 2c71e4e6) ----

    /// The audit-entries policy section, the endorsed shape (90d hot →
    /// archive-then-delete).
    const AUDIT_CLASS: &str = r#"{
        "classes": [],
        "audit_entries": {"window_days": 90, "disposition": "archive_then_delete"}
    }"#;

    #[test]
    fn audit_entries_class_absent_refuses_fail_closed() {
        // The endorsed ASSET table carries no audit_entries section — the
        // audit-prune verb must refuse execution (fail-closed).
        let p = endorsed();
        assert!(matches!(
            p.evaluate_audit_entries(90),
            PruneVerdict::Refused(_)
        ));
        assert!(matches!(
            p.evaluate_audit_entries(365),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn audit_entries_archive_then_delete_allowed_at_and_above_window() {
        let p: PrunePolicy = serde_json::from_str(AUDIT_CLASS).expect("audit class parses");
        assert_eq!(p.evaluate_audit_entries(90), PruneVerdict::Allowed);
        assert_eq!(p.evaluate_audit_entries(180), PruneVerdict::Allowed);
    }

    #[test]
    fn audit_entries_below_window_refused() {
        let p: PrunePolicy = serde_json::from_str(AUDIT_CLASS).expect("audit class parses");
        match p.evaluate_audit_entries(30) {
            PruneVerdict::Refused(reason) => {
                assert!(reason.contains("below the policy window"), "{reason}");
            }
            PruneVerdict::Allowed => panic!("sub-policy window must be refused"),
        }
    }

    #[test]
    fn audit_entries_archive_disposition_refused() {
        // disposition archive = the archive-only asset semantics — the
        // audit-prune verb archives-then-deletes; the policy must declare it.
        let p: PrunePolicy = serde_json::from_str(
            r#"{"audit_entries": {"window_days": 90, "disposition": "archive"}}"#,
        )
        .expect("parses");
        match p.evaluate_audit_entries(90) {
            PruneVerdict::Refused(reason) => {
                assert!(reason.contains("archive_then_delete"), "{reason}");
            }
            PruneVerdict::Allowed => panic!("archive disposition must refuse"),
        }
    }

    #[test]
    fn audit_entries_soft_delete_disposition_refused() {
        // Bare soft-delete of audit rows is prohibited (the audit trail is
        // the record) — the endorsed disposition is archive_then_delete.
        let p: PrunePolicy = serde_json::from_str(
            r#"{"audit_entries": {"window_days": 90, "disposition": "soft_delete"}}"#,
        )
        .expect("parses");
        assert!(matches!(
            p.evaluate_audit_entries(90),
            PruneVerdict::Refused(_)
        ));
    }

    #[test]
    fn asset_class_with_archive_then_delete_disposition_refused_by_asset_prune() {
        // The asset prune is soft-delete-only: an asset class carrying the
        // audit disposition must refuse there too (the audit-entries verb is
        // the only archive-then-delete executor).
        let p: PrunePolicy = serde_json::from_str(
            r#"{"classes": [{"tag_category": "type", "tag_value": "report", "window_days": 30, "disposition": "archive_then_delete"}]}"#,
        )
        .expect("parses");
        assert!(matches!(
            p.evaluate("type", "report", 30),
            PruneVerdict::Refused(_)
        ));
    }
}
