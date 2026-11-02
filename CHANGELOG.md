# Changelog

All notable changes to PDT are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
