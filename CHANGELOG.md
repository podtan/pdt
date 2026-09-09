# Changelog

All notable changes to PDT are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
