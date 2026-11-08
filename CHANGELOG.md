# Changelog

All notable changes to PDT are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.33] — 2026-11-08

### Added — git-style short-id prefix resolution on by-id READ paths (issue e9478d91)

- A well-formed UUID resolves directly (never a scan); a valid-hex id
  shorter than 36 chars that is a UNIQUE prefix of exactly one live asset
  resolves to it (case-insensitive — the response id is the canonical
  stored form); zero matches stay a body-bearing 404 (honest absence —
  never a guess); ≥2 matches answer **409 AMBIGUOUS_PREFIX** with the match
  count + the minimum sufficient length hint (the divergence length);
  malformed (non-hex, or ≥36 chars that isn't a valid UUID) answers
  **422 MALFORMED_ID** — a caller transcription error is never masked as
  absent. MUTATIONS STAY FULL-UUID: update/delete/tag writes keep the
  strict `get_by_id` path — a short id on a mutation 404s honestly. (The
  spec's teaching-422 on prefix mutations rides a future train bundled
  with the nghr 422-envelope arm — the throughflow math makes it a
  cross-service change, no-bundling.)
- Coverage: `GET /api/assets/{id}` + the relations reads
  (`GET /api/assets/{id}/relations`, `GET /api/assets/{id}/graph`) — the
  relations queries use the RESOLVED id. One implementation point
  (`AssetService::resolve`) kills the daily short-id 404 friction across
  every service (14 specimens on file: pdt_getAsset, nghr get_project,
  nghr list_project_tasks/list_project_workstreams).
- Repository surface: `AssetRepository::get_by_id_prefix(prefix)` — sqlite
  (LIKE with escaped wildcards, LIMIT 51 for the count) + mongo (anchored
  case-insensitive regex, limit 51). A caller-supplied `%`/`_`/`.` stays
  literal — never a full-table scan.
- Tenancy: the prefix query scans the TENANT'S OWN DB only (per-tenant
  pool) — a cross-band short id is absent, never a bypass (security-adjacent,
  tested explicitly).
- Conformance pins (sqlite, 8 new): unique-prefix resolves canonically,
  absent prefix → empty, wildcards stay literal, ambiguous → 409 with
  count + hint, malformed → 422, uppercase input resolves, cross-band
  prefix → absent.

## [0.3.32] — 2026-11-08

### Added — the dry-run response carries the STAGED audit-entries window (issue 2240beac)

- The audit-prune dry-run response gains `policy_window_days` +
  `policy_disposition` — the staged `audit_entries` class from
  PDT_PRUNE_POLICY_PATH, read at invocation (data, not code). The console
  modal's prefill source (the honest-surface law, farzan's fix shape):
  the modal prefills the policy of record, never a spec-era default.
  Absent section = the fields stay absent (the UI keeps its labeled
  fallback). Execution responses unchanged; dry-run only.
- Regression pin: `dry_run_response_carries_the_staged_policy_window`
  (the endorsed 30d shape + the absent-section fallback case).

## [0.3.31] — 2026-11-07

### Fixed — the audit-prune EXECUTION walk's per-chunk re-scan (issue 2c71e4e6) + the 002_groups restart 500-window (singleflight)

- FIX 1 — the execution archive walk (issue 2c71e4e6, paydar's 2026-11-07
  decode relayed by farzan): the replaced id-ASC cursor shape
  (`WHERE timestamp < ? AND id > ? ORDER BY id ASC LIMIT 500`) made every
  500-row page re-scan forward through the table on band-class volume —
  238,958 candidates ≈ 160 pages ≈ 25–40 min on prod (clients time out
  long before; the manual chunked path did the same volume in ~90s,
  ~20x faster). The walk now MATERIALIZES the candidate id set in ONE
  index-driven pass (`ids_before` — `WHERE timestamp < ?`, served by
  idx_audit_timestamp, no ORDER BY) and fetches full rows per
  explicit-id chunk (`list_by_ids` — PK-driven IN-list, exact, no
  re-scan). The candidate set is stable during the walk (rows created
  mid-walk are newer than the cutoff, never candidates); the
  archive-then-delete row-count verify still refuses a partial archive.
  `list_before` is superseded by the pair (removed — dead code is debt).
- FIX 2 — the 002_groups restart 500-window (paydar's 2026-11-07 sighting,
  2nd instance of the class, "yours to rule"): concurrent first-touches of
  one tenant store after a restart each ran the migration replay + FTS
  rebuild, and the loser's 002_groups write hit the winner's write lock
  and died at the ~5s busy_timeout (3×500 @ ~5.05s per restart window,
  self-healing when the rebuilds finished — ~60–90s of tenant 500s after
  EVERY pdt restart). TenantPoolManager now singleflights the open per
  instance UUID: concurrent first-touches await the SAME open and
  re-check the cache when it lands — the 500-window becomes a wait, never
  an error. Guard: concurrent_first_touch_singleflight_no_500 (8
  concurrent cold-manager opens).
- DOC — the audit-prune endpoint's post-run guidance (ruling 129f3707's
  acceptance addition): row counts are necessary but NOT sufficient — a
  row-count-only acceptance hides a no-reclamation prune (VACUUM delta-0
  on scattered deletes; the real reclaim comes from a restart's recovery
  consolidation after long-lived readers block WAL checkpoints). Every
  prune acceptance carries the DISK datum alongside the row counts.
- IT extended: the band-class volume walk (2500 old rows = 5 exact chunks
  of 500 — archived/deleted/post-count/verified + every row in the
  archive).

Gates at release: suite 199/0 (lib 138, audit_prune_it 2, auth_degraded 4,
gate_contract 6, sqlite_conformance 49), fmt 0, clippy 68 == HEAD baseline
68 (zero new lints).

## [0.3.30] — 2026-11-07

### Fixed — the >120s audit-prune hang on band-class volume (issue 2c71e4e6, CPO finding)

- ROOT CAUSE (planner-verified): the dry-run's sample query rode the
  id-ordered shape (`WHERE timestamp < ? ORDER BY id ASC LIMIT 10`) — a
  shape the sqlite planner resolves as a FULL TABLE SCAN when the cutoff
  matches few/zero rows (measured on a 1.9M-row fixture: 2.27s per scan on
  a fast box; minutes on the oversubscribed prod box — the >120s hang).
  The candidate count was already index-driven (`idx_audit_timestamp`,
  covering); the sample was the unbounded leg.
- FIX 1 — `sample_before` (new repo method, sqlite + mongo): the dry-run
  sample reads `ORDER BY timestamp DESC` — index-driven, bounded by LIMIT
  for ANY match count. The sample is a report (newest candidates first),
  not the archive walk; the id-ASC exactness contract stays with
  `list_before` alone.
- FIX 2 — 0-match short-circuit: `candidates == 0` returns the zero report
  immediately (dry-run AND execution) — no sample query, no archive walk,
  no delete, no audit entry. A window beyond the band's birth can no
  longer scan the table at all.
- PLANNER PIN (the guard): `audit_prune_sample_and_count_are_index_driven`
  asserts EXPLAIN QUERY PLAN uses `idx_audit_timestamp` and NEVER a bare
  `SCAN audit_entries` for the sample and count shapes — the planner
  choice is the defect, so the plan is the assertion.
- IT extended: 0-match dry-run + execution no-op phases (no archive file,
  rows intact, no audit entry); the dry-run sample pinned newest-first.
- Verified at band-class volume on the real binary: 0-match dry-run
  0.00s; 1.2M-candidate dry-run 0.04s; 1.2M-row execution 47s with
  archived == candidates == deleted, post_count 0, the archive line count
  exact, and the prune's own audit entry correct. The db file size does
  not shrink on DELETE (the CPO's acceptance-4 disk datum: VACUUM is the
  reclaim step — operator procedure, never in-request: an in-request
  VACUUM holds the write lock for minutes = the band's writes block).

## [0.3.29] — 2026-11-07

### Added — audit-entries retention prune, ARCHIVE-THEN-DELETE (issue 2c71e4e6, owner-directed)

- `POST /api/maintenance/audit-prune {older_than_days, dry_run}`: the
  missing lifecycle leg for the `audit_entries` TABLE (INSERT-only since
  inception; the THQ leaf band a4613285 grew to 1.88M rows / 2.07G — the
  namespace's 2G lever). The existing asset prune (soft-delete via
  `AssetService::delete`) cannot touch the table — an audit entry is not
  an asset.
- ARCHIVE-THEN-DELETE, not soft-delete: `dry_run=false` exports EVERY
  candidate row to a dated JSONL archive
  (`{band dir}/audit-archive-<ts>.jsonl`, the archive lives next to the
  store it documents; `PDT_AUDIT_ARCHIVE_DIR` overrides) and fsyncs it
  BEFORE any delete; the delete is REFUSED unless archived == candidates
  (a partial archive is removed, never kept, never trusted); post-delete
  count must be 0; the prune itself rides ONE audit entry (action=delete,
  entity_type=audit_entries) carrying the archive path + counts.
- `dry_run=true`: the numbers only — candidates count + 10-row sample +
  the planned archive home + the policy verdict. Zero writes, zero audits.
- Policy wiring: the audit-entries class is a DEDICATED
  `PDT_PRUNE_POLICY_PATH` section —
  `{"audit_entries": {"window_days": 90, "disposition": "archive_then_delete"}}`.
  Absent section, disposition `archive`/`soft_delete`, or window below
  policy = 403 (fail-closed). `Disposition` gained `ArchiveThenDelete`;
  the asset-prune evaluator refuses it explicitly (the asset prune stays
  soft-delete-only).
- Gates: same fail-closed set as the asset prune — admin (token role
  claim or registry global_role), `PDT_PRUNE_ENABLED` default-OFF wire
  gate, policy-cleared window. Dry-run always answers. Execution stays
  disabled until the CPO/owner sight the dry-run numbers (ruling
  8d591e85).
- Repository: `AuditRepository` grew `count_before` / `list_before` /
  `delete_before` (sqlite + mongo). The archive walk paginates on the
  unique id PK ASC — exact, no skips/duplicates.
- Tests: 6 policy tests + the end-to-end IT (`tests/audit_prune_it.rs`):
  gate default-off, policy absent, below-window refused, dry-run
  zero-writes, archive-then-delete verify, no-silent-delete pin.

## [0.3.28] — 2026-11-02

### Added — the DESIGNED seed mechanism for instance-admins (CPO ruling on defect c0c606c6, option (b))

- `PDT_SEED_INSTANCE_ADMINS` (comma-separated token subs): bootstrap
  principals are seeded into EVERY opened store's instance-admins group
  at store open — the global store at boot (main.rs) and every
  workspace/child leaf at pool open (tenant.rs). An empty admin group at
  boot is born-invisible governance (the grey-icons class); prod was
  never seeded while the IT env was (the IT/prod parity gap that left
  the tocpi propagate-admins backfill — work order ed35e52a — blocked
  with all leaf writes 403 at the groups surface).
- Idempotent: an already-member sub is SKIPPED (no duplicate membership
  row, no duplicate audit entry on re-boot). Audited: each landing
  writes ONE audit entry (action=AddMember, `seed: true`, actor
  `system`) via the same audited path as every other group mutation.
- Per-sub failures are WARN-loud and NON-FATAL — the store still opens
  (zero downtime); residue stays visible in the journal and via the
  tocpi admin-drift detector.
- Backend-agnostic: rides the GroupService/repository trait layer (sqlite
  and mongo today, postgres later).
- Converges with task 33b6d070 (the Phase 2b migration): one designed
  boot-time seeding mechanism family serves both the bootstrap-principal
  seed and the groups-table migration (coordination per the ruling).

### Fixed — lying policy comments (same ruling pass)

- `policies/rbac.cedar` header comments referenced the retired IdP
  hostname (`idm.tanbal.ir`) and a `pdt_admins` group that no longer
  exists — comments that lie are fail-silent documentation. Corrected to
  the pdt-api claim-map transport (`role:pdt-api-admins@<idp-host>` →
  `admin`) with the host left to deploy config.

### Regression pins (3, standalone)

- `seed_lands_audits_and_heals_the_bypass` — the seeded subs are
  instance-admins, the audit entries carry the seed marker + actor, and
  the seeded member's admin bypass resolves at read time (the prod-gap
  acceptance shape).
- `seed_is_idempotent` — re-seed adds no duplicate membership rows and
  no duplicate audit entries.
- `seed_empty_and_whitespace_noop` — empty list / whitespace-only subs
  seed nothing.

## [0.3.27] — 2026-11-02

### Added — Cedar list-filter trace (issue 51645197; the fail-silent class on the LIST path)

- `filter_by_permission` / `filter_by_permission_batched` now LOG the
  per-asset drop (`Cedar filtered asset {id} for user {sub} on action
  {action}: denied`, debug level) — previously `Ok(false)` filtered with
  no log and only evaluation ERRORS logged, so a filtered list was
  indistinguishable from an empty store (the 40ccddef mehrdad/Eynak
  diagnosis took hours because the server saw nothing).
- Per-request summary (`Cedar list filter: kept X of Y assets for user
  {sub} on instance {id} (action {action})`, warn level) emitted by the
  resolved wrappers ONLY when drops occurred — zero-drop lists stay
  quiet. The GET path (`check_permission`) already logged denies; the
  list-path asymmetry is closed.
- Regression tests (`filter_logging_tests`): the drop leaves a trace
  naming asset + user (per-row + batched), and the summary warns only on
  drops.

## [0.3.26] — 2026-11-02

### Added — ADOPT-BY-ID: same-uuid materialization (CPO ruling 6d86ee70; the Path A adopt's first-class path)

- `POST /api/assets/{id}/adopt` — materialize an asset with a SPECIFIED
  uuid. The Path A adopt (lock f70646ee, specimen 78398674 "Profile:
  Ravand-nox") was blocked: a plain create mints a NEW uuid (the duplicate
  the lock forbids) and direct-sqlite restore is barred by the standing
  no-direct-sqlite law — this operation is the first-class path that
  replaces it.
- SHAPE (the ruling's conditions, machine-enforced):
  - **additive**: no change to create/birth semantics; plain create keeps
    minting new uuids
  - **duplicate-barred**: refuses loudly (409) if the uuid exists live OR
    soft-deleted (the PK is the hard bar — an adopt over a soft-deleted
    row would be a resurrection, a different operation)
  - **custody**: platform-admin only (role=admin); the handler checks
  - **audited ATOMICALLY**: the audit entry (action=adopt, provenance in
    changes) and the asset land in ONE sqlite transaction — a failed
    audit write DENIES the adopt (unaudited materialization = silent
    widening, fail-loud); the mongo backend compensates (delete + deny,
    loud error)
  - **provenance REQUIRED** (400 if absent): the call names the banked
    source; it lands in the audit entry
- GUARDS (release checklist v3, standalone verbatim — 4 new tests in
  `adopt_guards`): duplicate-refusal (one copy stays) · audit-write-denies
  (the audit table dropped → the adopt is denied AND the asset is absent —
  the transaction rolled back) · happy path (the adopt lands + the audit
  entry carries the provenance) · provenance-required.
- Timestamps: the adopt materializes with NOW timestamps (it is a NEW
  materialization event, not a byte-copy of the backup row); the audit
  entry records the provenance. The custody holder (Hafez) executes the
  locked Path A adopt.

## [0.3.25] — 2026-11-02

### Added — PDT-native per-instance groups + cedar merge (workstream 69b4bb4c, tasks a9f91cfb + 147a17fc; owner directive 2026-11-02 relayed by Farzan)

- **Phase 1a (a9f91cfb)**: per-INSTANCE groups — `groups` + `group_members`
  tables in every instance store (migration `002_groups.sql`, idempotent,
  applied at every boot) + a distinguished `instance-admins` group seeded in
  every store (stable id `00000000-0000-0000-0000-0000000000a1`; the name is
  reserved at create; deletion refused). Management via REST
  (`/api/groups` — create/list/get/delete + members add/remove/list + the
  read-time resolve surface `/api/groups/member/{member_id}`), instant and
  audited in `audit_entries` (`add_member` / `remove_member` actions). No
  GitOps, no Kanidm admin token. Backend-agnostic: the `GroupRepository`
  trait (sqlite + mongo implementations; postgres later = one new impl + a
  backend arm + a dialect migration file), and a backend-agnostic
  `ensure_instance_admins_group` bootstrap at every store open.
- **Phase 1b (147a17fc)**: the cedar evaluator MERGES —
  `effective_groups = pep-fetched Kanidm groups ∪ PDT-native groups`
  (resolved at read time by principal id from the new tables). Union
  semantics: a failure in either source degrades to the other, never to
  zero. Admin bypass: a principal in the instance-admins group sees all
  assets within that instance, AUDITED (`admin_bypass` entries; a failed
  audit write denies — an unaudited bypass is a silent widening). Band
  matching resolves MEMBERSHIP, not string forms — bands store group
  names; the SPN-vs-UUID band-form mismatch class dies.
- **365be03f interplay**: a degraded TOKEN (enrichment landed empty) no
  longer refuses filtered reads when PDT-native memberships resolve —
  Degraded answers 503 only when BOTH sources are empty (zero = zero,
  honestly).

### Why

The live casualty (defect 40ccddef): mehrdad.haddad saw "No assets found"
on the Eynak PDT view while the store + bands were healthy — the failure
was in the token-claim transport (pep enrichment / mint staleness / mint
gaps / the JWKS single-key rotation treadmill 8141eda7). The merge makes
that failure mode harmless the moment PDT-native groups carry real
memberships. The visibility model itself is owner-ratified and stays:
visibility=team + user in the group band → visible; admin group → see all.
We are fixing the transport, not the model.

### Regression pins (the CLASS — release checklist v3)

7 new tests (`native_groups_merge_tests`): read-time resolution without
token groups · union semantics (both sources, either-source bands) · admin
bypass sees-all + audited · names-not-IDs (UUID-form AND SPN-form member
ids match a name-form band) · no-widening (no membership still denied) ·
the distinguished group seeded + protected · instance-admins manage
groups.

## [0.3.7] - 2026-10-11

### Changed
- pep 0.5.7 → 0.5.8 (Bonyan's fail-closed enrichment; owner fix order payam
  a77cd19c): userinfo failures no longer swallow to `Ok(())` — they surface as
  `PepError::AuthDegraded { retryable }`. The auth middleware now branches:
  - **retryable** degradation (IdP 5xx/429/transport) → `503
    {"error":"auth_degraded","retryable":true}` — the issue-365be03f contract
    filtered reads key on; the same request may succeed on retry.
  - **non-retryable** degradation (4xx invalid_token — terminal for the token)
    and other enrichment failures → the standing b82a1925
    `401 token_expired retryable` contract with `WWW-Authenticate` challenge;
    a caller's reactive refresh cures it in one request.
- The post-enrichment classification layer (v0.3.6) stays as defense in depth
  for the "200-but-no-attributes" case, which pep 0.5.8 still returns `Ok`.

### Added
- IT gate now covers both degraded flavors on the real binary: dead-token 401
  with challenge + transient 503 contract, each on a fresh sub (userinfo-cache
  dodge), plus the fail-closed pre-handler asset-GET assertion.

## [0.3.6] - 2026-10-11

### Fixed
- **Visibility-filtered reads no longer lie by omission** (issue 365be03f, production
  gate): when token-claims enrichment lands empty (Kanidm access tokens carry no
  identity claims; pep treats userinfo HTTP errors as non-fatal Ok), the
  zero-attribute principal used to make `GET /api/search` and `GET /api/assets`
  answer 200-with-empty — indistinguishable from genuine absence (payam F1,
  2026-10-07). Both endpoints now answer `503` with
  `{"error":"auth_degraded","retryable":true}` when the request is auth-degraded.
  Zero-rows means zero-rows.

### Changed
- pep 0.5.0 → 0.5.7: JWKS rotation force-refresh with storm guard, token-provider
  honors real JWT exp (+ `TokenProvider::invalidate`), Cedar handles Kanidm's
  array-form role claim, 60s JWT validation leeway.

### Added
- Post-enrichment request state (`Enriched`/`Degraded`) on the auth middleware,
  journaled at WARN when degraded.
- Real-binary IT gate `auth_degraded_it`: in-process mock IdP whose userinfo flips
  to 400 mid-flight; asserts 503 (never 200-empty) on both filtered reads, honest
  deny on asset-GET, and recovery — fresh sub per phase to dodge pep's per-sub
  userinfo cache.

### Style
- Repo-wide rustfmt pass (rustfmt toolchain drift since 0.3.5; no behavior change).

## [0.3.5] - 2026-09-09

### Added
- Crate publish metadata, repository URL, and dual MIT/Apache-2.0 license files.

### Changed
- README rewritten for the current feature set: multi-instance tenancy, SQLite root
  backend, free-form tags, and a condensed API reference.
- Changelog restructured.

### Fixed
- Pep-sourced 401 responses (e.g. JWT validation failures) now carry the
  `WWW-Authenticate` challenge header, matching the other 401 paths.

### Security
- Removed a credential-shaped example value from the README.

## [0.3.4] - 2026-09-08

### Fixed
- Search reduced user input to quoted FTS5 phrases — raw queries could be interpreted as
  FTS5 syntax (e.g. a bare `token:` column filter), failing with a database error.
- Default build (`mongodb-backend`) compiles again: `sqlx` is load-bearing for instance
  routing and is no longer optional.

## [0.3.3] - 2026-09-07

### Fixed
- Token enrichment failure now answers `401` with a `WWW-Authenticate: Bearer
  error="invalid_token"` challenge and a retryable JSON body instead of a silent
  role-less principal falling through to authorization `403`s.

## [0.3.2] - 2026-08-14

### Added
- Nested workspace instances: one SQLite database per workspace/agent/company leaf,
  provisioned under a parent instance.

## [0.3.1] - 2026-08-14

### Added
- `has_agent` relation type.

### Removed
- Stray SQLite database files accidentally committed; `*.db` patterns added to `.gitignore`.

## [0.3.0] - 2026-08-08

### Added
- Multi-instance tenancy: `X-Instance-Id` header routing to per-instance SQLite
  databases under `PDT_INSTANCES_DIR`, with provision/list/delete/tree instance APIs.
- `has_instance` relation type.

### Changed
- Instance-type assets are protected from modification by non-admin principals (Cedar).

## [0.2.0] - 2025-12-25

### Added
- OIDC resource-server authentication via [pep](https://crates.io/crates/pep): JWT
  bearer validation with JWKS caching, claim extraction, and optional userinfo enrichment.
- User attribution (`created_by` / `updated_by`) on assets and collections, plus a
  request-audit trail.
- Cedar authorization policies (compile-time embedded, configurable via `CEDAR_*` vars).

### Breaking
- All write endpoints require a valid bearer token.

## [0.1.0] - 2025-12-24

### Added
- Initial release: asset/relation/collection CRUD with soft deletes, free-form tagging,
  full-text search, MongoDB persistence, audit log, OpenAPI spec, health endpoint.

[Unreleased]: https://github.com/podtan/pdt/compare/v0.3.5...HEAD
[0.3.5]: https://github.com/podtan/pdt/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/podtan/pdt/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/podtan/pdt/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/podtan/pdt/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/podtan/pdt/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/podtan/pdt/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/podtan/pdt/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/podtan/pdt/releases/tag/v0.1.0
